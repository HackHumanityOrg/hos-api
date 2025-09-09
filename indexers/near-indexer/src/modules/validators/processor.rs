use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono;
use entities::*;
use near_lake_framework::near_indexer_primitives::views::{
    ActionView, ReceiptEnumView, StateChangeCauseView,
};
use near_primitives::views::{validator_stake_view::ValidatorStakeView, ChunkHeaderView};
use rust_decimal::{Decimal, RoundingStrategy};
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter, Set, TransactionTrait,
};
use serde_json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::core::{build_block, cursor};
use crate::IndexerModule;

const MODULE_NAME: &str = "validators";

pub struct ValidatorsModule {
    enabled: bool,
    last_epoch_id: Arc<RwLock<Option<String>>>,
    epoch_validators: Arc<RwLock<HashMap<String, HashSet<String>>>>, // epoch_id -> validator_ids
    validator_performance: Arc<RwLock<HashMap<(String, String), ValidatorEpochPerformance>>>, // (epoch_id, validator_id) -> performance
    staking_pools: Arc<RwLock<HashSet<String>>>, // Known staking pool contract IDs
    rpc_updater_config: Option<common::config::RpcUpdaterConfig>, // RPC updater configuration
}

#[derive(Debug, Clone, Default)]
struct ValidatorEpochPerformance {
    blocks_produced: u32,
    chunks_produced: u32,
    chunks_per_shard: HashMap<u64, u32>,
    expected_blocks: u32,
    expected_chunks: u32,
}

#[derive(Debug, Clone)]
struct ProposalContext {
    height: i64,
    epoch_id: String,
    timestamp: i64,
    source: String,
    shard_id: Option<i32>,
}

#[derive(Debug, Clone)]
struct DelegationAction {
    delegator_id: String,
    pool_id: String,
    action_type: String,
    amount: Option<u128>,
    args: Option<serde_json::Value>,
    transaction_hash: String,
    receipt_id: String,
    block_height: i64,
    timestamp: i64,
    epoch_id: String,
    success: bool,
}

impl ValidatorsModule {
    /// Safely convert u128 to Decimal using string conversion (same as accounts module)
    fn safe_u128_to_decimal(value: u128) -> Decimal {
        Decimal::from_str_exact(&value.to_string()).unwrap_or_else(|_| {
            warn!(value = %value, "u128 value conversion to Decimal failed, using zero");
            Decimal::ZERO
        })
    }

    pub fn new(config: &common::config::AppConfig) -> Self {
        let enabled = match config {
            common::config::AppConfig::Indexer(idx) => {
                let near_config = idx.near.as_ref();

                // Check if NEAR indexing is enabled at all
                let near_enabled = near_config.map(|c| c.chain != "disabled").unwrap_or(false);

                if !near_enabled {
                    return Self {
                        enabled: false,
                        last_epoch_id: Arc::new(RwLock::new(None)),
                        epoch_validators: Arc::new(RwLock::new(HashMap::new())),
                        validator_performance: Arc::new(RwLock::new(HashMap::new())),
                        staking_pools: Arc::new(RwLock::new(HashSet::new())),
                        rpc_updater_config: None,
                    };
                }

                // Get validators module configuration
                let validators_config = near_config
                    .and_then(|nc| nc.modules.as_ref())
                    .and_then(|mc| mc.validators.as_ref());

                let enabled = validators_config.map(|vc| vc.enabled).unwrap_or(true);
                let rpc_updater_config = validators_config.and_then(|vc| vc.rpc_updater.clone());

                (enabled, rpc_updater_config)
            }
            _ => (false, None),
        };

        let (enabled, rpc_updater_config) = enabled;

        Self {
            enabled,
            last_epoch_id: Arc::new(RwLock::new(None)),
            epoch_validators: Arc::new(RwLock::new(HashMap::new())),
            validator_performance: Arc::new(RwLock::new(HashMap::new())),
            staking_pools: Arc::new(RwLock::new(HashSet::new())),
            rpc_updater_config,
        }
    }

    async fn record_block_producer(
        &self,
        producer_id: &str,
        height: i64,
        epoch_id: &str,
        timestamp: i64,
        gas_price: u128,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let block_producer = near_block_producers::ActiveModel {
            block_height: Set(height),
            producer_id: Set(producer_id.to_string()),
            epoch_id: Set(epoch_id.to_string()),
            timestamp: Set(timestamp),
            gas_price: Set(Self::safe_u128_to_decimal(gas_price)),
        };

        // Use on_conflict_do_nothing to handle duplicate inserts from parallel processing
        // This generates: INSERT ... ON DUPLICATE KEY UPDATE block_height = block_height
        near_block_producers::Entity::insert(block_producer)
            .on_conflict_do_nothing()
            .exec(db)
            .await?;
        Ok(())
    }

    async fn record_chunk_producers(
        &self,
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let b = build_block::build_block(msg);
        let height = b.header.height;
        let epoch_id = b.header.epoch_id.clone();
        let timestamp = b.header.timestamp_nanosec as i64;

        let mut models = Vec::new();
        for (idx, cp) in b.chunk_producers.iter().enumerate() {
            if let Some(producer) = &cp.producer {
                models.push(near_chunk_producers::ActiveModel {
                    id: NotSet,
                    block_height: Set(height),
                    shard_id: Set(idx as i32),
                    producer_id: Set(producer.clone()),
                    epoch_id: Set(epoch_id.clone()),
                    timestamp: Set(timestamp),
                });
            }
        }
        if !models.is_empty() {
            near_chunk_producers::Entity::insert_many(models)
                .on_conflict_do_nothing()
                .exec(db)
                .await?;
        }
        Ok(())
    }

    async fn record_validator_proposals(
        &self,
        proposals: &[ValidatorStakeView],
        context: &ProposalContext,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let mut proposal_models = Vec::new();

        for proposal in proposals {
            let account_id = proposal.clone().take_account_id().to_string();
            let (public_key, stake) = match proposal {
                ValidatorStakeView::V1(v1) => (v1.public_key.to_string(), v1.stake),
            };

            // Insert/update validator record
            self.upsert_validator(
                &account_id,
                &public_key,
                context.height,
                context.timestamp,
                db,
            )
            .await?;

            // Insert proposal record
            proposal_models.push(near_validator_proposals::ActiveModel {
                id: NotSet,
                block_height: Set(context.height),
                account_id: Set(account_id),
                public_key: Set(public_key),
                stake: Set(Self::safe_u128_to_decimal(stake)),
                epoch_id: Set(context.epoch_id.clone()),
                timestamp: Set(context.timestamp),
                source: Set(context.source.clone()),
                shard_id: Set(context.shard_id),
            });
        }

        if !proposal_models.is_empty() {
            near_validator_proposals::Entity::insert_many(proposal_models)
                .exec(db)
                .await?;
        }

        Ok(())
    }

    async fn upsert_validator(
        &self,
        account_id: &str,
        public_key: &str,
        height: i64,
        timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // Try to find existing validator
        let existing = near_validators::Entity::find()
            .filter(near_validators::Column::AccountId.eq(account_id))
            .one(db)
            .await?;

        if let Some(validator) = existing {
            // Update existing validator
            let mut active: near_validators::ActiveModel = validator.into();
            active.last_seen_height = Set(Some(height));
            active.last_seen_timestamp = Set(Some(timestamp));
            active.update(db).await?;
        } else {
            // Insert new validator
            let new_validator = near_validators::ActiveModel {
                account_id: Set(account_id.to_string()),
                public_key: Set(Some(public_key.to_string())),
                first_seen_height: Set(height),
                first_seen_timestamp: Set(timestamp),
                last_seen_height: Set(Some(height)),
                last_seen_timestamp: Set(Some(timestamp)),
            };
            near_validators::Entity::insert(new_validator)
                .on_conflict_do_nothing()
                .exec(db)
                .await?;
        }

        Ok(())
    }

    async fn record_validator_approvals(
        &self,
        approvals: &[Option<Box<near_crypto::Signature>>],
        height: i64,
        epoch_id: &str,
        timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let total_approvals = approvals.iter().filter(|a| a.is_some()).count() as i32;
        let total_slots = approvals.len() as i32;

        let approval_record = near_validator_approvals::ActiveModel {
            block_height: Set(height),
            total_approvals: Set(total_approvals),
            total_slots: Set(total_slots),
            epoch_id: Set(epoch_id.to_string()),
            timestamp: Set(timestamp),
        };

        near_validator_approvals::Entity::insert(approval_record)
            .on_conflict_do_nothing()
            .exec(db)
            .await?;
        Ok(())
    }

    async fn record_validator_rewards(
        &self,
        validator_reward: u128,
        block_producer: &str,
        height: i64,
        epoch_id: &str,
        timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        if validator_reward == 0 {
            return Ok(());
        }

        // Record total reward for the epoch (aggregated)
        let total_reward_model = entities::near_validator_rewards::ActiveModel {
            id: NotSet,
            validator_id: Set("*total*".to_string()),
            epoch_id: Set(epoch_id.to_string()),
            block_height: Set(height),
            reward_amount: Set(Self::safe_u128_to_decimal(validator_reward)),
            delegator_rewards: Set(rust_decimal::Decimal::ZERO), // Not available at block level
            commission_earned: Set(rust_decimal::Decimal::ZERO), // Not available at block level
            commission_rate: Set(rust_decimal::Decimal::ZERO),   // Not available at block level
            timestamp: Set(timestamp),
            source: Set("block".to_string()),
            shard_id: NotSet,
        };

        near_validator_rewards::Entity::insert(total_reward_model)
            .on_conflict_do_nothing()
            .exec(db)
            .await?;

        // Record individual block producer reward
        // In NEAR, the block producer gets the validator reward for producing this block
        let producer_reward_model = entities::near_validator_rewards::ActiveModel {
            id: NotSet,
            validator_id: Set(block_producer.to_string()),
            epoch_id: Set(epoch_id.to_string()),
            block_height: Set(height),
            reward_amount: Set(Self::safe_u128_to_decimal(validator_reward)),
            delegator_rewards: Set(rust_decimal::Decimal::ZERO), // Individual delegator rewards not available
            commission_earned: Set(rust_decimal::Decimal::ZERO), // Commission details not available at block level
            commission_rate: Set(rust_decimal::Decimal::ZERO), // Commission rate not available at block level
            timestamp: Set(timestamp),
            source: Set("block".to_string()),
            shard_id: NotSet,
        };

        near_validator_rewards::Entity::insert(producer_reward_model)
            .on_conflict_do_nothing()
            .exec(db)
            .await?;

        debug!(
            height = height,
            epoch_id = %epoch_id,
            block_producer = %block_producer,
            validator_reward = %validator_reward,
            "Validator rewards recorded for block producer"
        );

        Ok(())
    }

    async fn record_chunk_validator_rewards(
        &self,
        chunk: &ChunkHeaderView,
        producer_id: Option<&str>,
        shard_id: usize,
        height: i64,
        epoch_id: &str,
        timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let validator_reward = chunk.validator_reward;
        if validator_reward == 0 || producer_id.is_none() {
            return Ok(());
        }

        let producer_id = producer_id.unwrap();

        // Record chunk producer reward
        let chunk_reward_model = entities::near_validator_rewards::ActiveModel {
            id: NotSet,
            validator_id: Set(producer_id.to_string()),
            epoch_id: Set(epoch_id.to_string()),
            block_height: Set(height),
            reward_amount: Set(Self::safe_u128_to_decimal(validator_reward)),
            delegator_rewards: Set(rust_decimal::Decimal::ZERO), // Not available at chunk level
            commission_earned: Set(rust_decimal::Decimal::ZERO), // Not available at chunk level
            commission_rate: Set(rust_decimal::Decimal::ZERO),   // Not available at chunk level
            timestamp: Set(timestamp),
            source: Set("chunk".to_string()),
            shard_id: Set(Some(shard_id as i32)),
        };

        near_validator_rewards::Entity::insert(chunk_reward_model)
            .on_conflict_do_nothing()
            .exec(db)
            .await?;

        debug!(
            height = height,
            shard_id = shard_id,
            validator_id = %producer_id,
            reward_amount = %validator_reward,
            "Recorded chunk validator reward"
        );

        Ok(())
    }

    async fn record_epoch_validators(
        &self,
        epoch_id: &str,
        validators: &[ValidatorStakeView],
        epoch_start_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        debug!(
            epoch_id = epoch_id,
            validators_count = validators.len(),
            "Recording epoch validators"
        );

        let mut validator_account_ids = HashSet::new();

        for validator in validators {
            let validator_id = validator.account_id();
            validator_account_ids.insert(validator_id.to_string());

            // Determine validator type based on stake (top 100 are block producers)
            let validator_type = if validators.len() <= 100
                || validators
                    .iter()
                    .position(|v| v.account_id() == validator_id)
                    .unwrap_or(999)
                    < 100
            {
                "block_producer"
            } else {
                "chunk_validator"
            };

            // Check if this is a new validator
            let existing_validator = near_validators::Entity::find_by_id(validator_id.to_string())
                .one(db)
                .await?;
            let is_new_validator = existing_validator.is_none();

            // Record in near_validators if new
            if is_new_validator {
                let validator_record = near_validators::ActiveModel {
                    account_id: Set(validator_id.to_string()),
                    public_key: Set(match validator {
                        ValidatorStakeView::V1(v1) => Some(v1.public_key.to_string()),
                    }),
                    first_seen_height: Set(epoch_start_height),
                    first_seen_timestamp: Set(0), // Will be updated with actual timestamp
                    last_seen_height: Set(Some(epoch_start_height)),
                    last_seen_timestamp: Set(Some(0)),
                };
                near_validators::Entity::insert(validator_record)
                    .on_conflict_do_nothing()
                    .exec(db)
                    .await
                    .context("Failed to insert validator record")?;
            } else {
                // Update last seen
                let mut validator: near_validators::ActiveModel =
                    existing_validator.unwrap().into();
                validator.last_seen_height = Set(Some(epoch_start_height));
                validator.last_seen_timestamp = Set(Some(0));
                validator
                    .update(db)
                    .await
                    .context("Failed to update validator last seen")?;
            }

            // Record epoch validator info
            let epoch_validator = near_epoch_validators::ActiveModel {
                epoch_id: Set(epoch_id.to_string()),
                validator_account_id: Set(validator_id.to_string()),
                stake_amount: Set(match validator {
                    ValidatorStakeView::V1(v1) => Self::safe_u128_to_decimal(v1.stake),
                }),
                public_key: Set(match validator {
                    ValidatorStakeView::V1(v1) => Some(v1.public_key.to_string()),
                }),
                validator_type: Set(validator_type.to_string()),
                is_new_validator: Set(if is_new_validator { 1 } else { 0 }),
                seat_price: Set(None),   // Will be calculated separately
                expected_blocks: Set(0), // Will be calculated based on validator type
                expected_chunks: Set(0), // Will be calculated based on shard assignment
                expected_chunks_per_shard: Set(None),
            };

            near_epoch_validators::Entity::insert(epoch_validator)
                .on_conflict_do_nothing()
                .exec(db)
                .await
                .context("Failed to insert epoch validator")?;
        }

        // Update in-memory tracking
        let mut epoch_validators = self.epoch_validators.write().await;
        epoch_validators.insert(epoch_id.to_string(), validator_account_ids);

        Ok(())
    }

    async fn check_epoch_transition(
        &self,
        current_epoch_id: &str,
        height: i64,
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let mut last_epoch = self.last_epoch_id.write().await;

        match last_epoch.as_ref() {
            Some(previous_epoch) if previous_epoch != current_epoch_id => {
                info!(
                    height = height,
                    old_epoch = previous_epoch,
                    new_epoch = current_epoch_id,
                    "Epoch transition detected, recording validator performance"
                );

                // Finalize performance for previous epoch
                self.finalize_epoch_performance(previous_epoch, height - 1, db)
                    .await?;

                // Record new epoch validators if we have proposals
                if !msg.block.header.validator_proposals.is_empty() {
                    self.record_epoch_validators(
                        current_epoch_id,
                        &msg.block.header.validator_proposals,
                        height,
                        db,
                    )
                    .await?;
                }
            }
            None => {
                info!(
                    height = height,
                    epoch_id = current_epoch_id,
                    "First epoch detected, initializing validator tracking"
                );

                // Initialize epoch validators if we have proposals
                if !msg.block.header.validator_proposals.is_empty() {
                    self.record_epoch_validators(
                        current_epoch_id,
                        &msg.block.header.validator_proposals,
                        height,
                        db,
                    )
                    .await?;
                }
            }
            _ => {
                // Same epoch, continue tracking
            }
        }

        *last_epoch = Some(current_epoch_id.to_string());
        Ok(())
    }

    async fn finalize_epoch_performance(
        &self,
        epoch_id: &str,
        epoch_end_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let performance = self.validator_performance.read().await;

        for ((perf_epoch_id, validator_id), perf) in performance.iter() {
            if perf_epoch_id != epoch_id {
                continue;
            }

            // Calculate uptime percentage using NEAR formula
            let block_rate = if perf.expected_blocks > 0 {
                perf.blocks_produced as f64 / perf.expected_blocks as f64
            } else {
                1.0
            };

            let chunk_rate = if perf.expected_chunks > 0 {
                perf.chunks_produced as f64 / perf.expected_chunks as f64
            } else {
                1.0
            };

            let uptime = (block_rate + chunk_rate) / 2.0 * 100.0;

            // Determine if kicked out (below 80-90% threshold)
            let is_kicked_out = uptime < 80.0;
            let kickout_reason = if is_kicked_out {
                Some("low_performance".to_string())
            } else {
                None
            };

            // Get epoch start height
            let epoch_start_height = near_epochs::Entity::find()
                .filter(near_epochs::Column::EpochId.eq(epoch_id))
                .one(db)
                .await?
                .map(|e| e.start_height)
                .unwrap_or(0);

            let performance_record = near_validator_performance::ActiveModel {
                epoch_id: Set(epoch_id.to_string()),
                validator_account_id: Set(validator_id.clone()),
                num_blocks_produced: Set(perf.blocks_produced as i32),
                num_blocks_expected: Set(perf.expected_blocks as i32),
                num_chunks_produced: Set(perf.chunks_produced as i32),
                num_chunks_expected: Set(perf.expected_chunks as i32),
                num_chunks_produced_per_shard: Set(Some(
                    serde_json::to_value(&perf.chunks_per_shard).unwrap(),
                )),
                uptime_percentage: Set(Some(
                    Decimal::from_f64_retain(uptime)
                        .unwrap_or(Decimal::ZERO)
                        .round_dp_with_strategy(2, RoundingStrategy::MidpointNearestEven),
                )),
                is_kicked_out: Set(if is_kicked_out { 1 } else { 0 }),
                kickout_reason: Set(kickout_reason),
                epoch_start_height: Set(epoch_start_height),
                epoch_end_height: Set(Some(epoch_end_height)),
            };

            near_validator_performance::Entity::insert(performance_record)
                .on_conflict_do_nothing()
                .exec(db)
                .await
                .context("Failed to insert validator performance")?;
        }

        // Clear performance data for finalized epoch
        let mut perf_guard = self.validator_performance.write().await;
        perf_guard.retain(|(perf_epoch_id, _), _| perf_epoch_id != epoch_id);

        // Calculate validator APY for this completed epoch
        let epoch_start_height = near_epochs::Entity::find()
            .filter(near_epochs::Column::EpochId.eq(epoch_id))
            .one(db)
            .await?
            .map(|e| e.start_height)
            .unwrap_or(0);

        self.calculate_validator_apy(
            epoch_id,
            epoch_start_height,
            epoch_end_height,
            chrono::Utc::now().timestamp(),
            db,
        )
        .await
        .context("Failed to calculate validator APY")?;

        debug!(
            epoch_id = epoch_id,
            epoch_end_height = epoch_end_height,
            "Finalized epoch validator performance and calculated APY"
        );

        Ok(())
    }

    async fn track_block_production(&self, producer_id: &str, epoch_id: &str) -> Result<()> {
        let mut performance = self.validator_performance.write().await;
        let key = (epoch_id.to_string(), producer_id.to_string());

        let perf = performance.entry(key).or_default();
        perf.blocks_produced += 1;

        Ok(())
    }

    async fn track_chunk_production(
        &self,
        producer_id: &str,
        shard_id: u64,
        epoch_id: &str,
    ) -> Result<()> {
        let mut performance = self.validator_performance.write().await;
        let key = (epoch_id.to_string(), producer_id.to_string());

        let perf = performance.entry(key).or_default();
        perf.chunks_produced += 1;
        *perf.chunks_per_shard.entry(shard_id).or_insert(0) += 1;

        Ok(())
    }

    async fn record_slashing_event(
        &self,
        validator_id: &str,
        epoch_id: &str,
        block_height: i64,
        slash_amount: u128,
        slash_reason: &str,
        timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let slashing_event = near_validator_slashing::ActiveModel {
            id: NotSet,
            validator_account_id: Set(validator_id.to_string()),
            epoch_id: Set(epoch_id.to_string()),
            block_height: Set(block_height),
            slash_amount: Set(Self::safe_u128_to_decimal(slash_amount)),
            slash_reason: Set(slash_reason.to_string()),
            timestamp: Set(timestamp),
        };

        near_validator_slashing::Entity::insert(slashing_event)
            .exec(db)
            .await
            .context("Failed to insert slashing event")?;

        info!(
            validator = validator_id,
            epoch = epoch_id,
            amount = %slash_amount,
            reason = slash_reason,
            "Recorded validator slashing event"
        );

        Ok(())
    }

    /// Check if an account ID is a staking pool based on known patterns
    fn is_staking_pool(&self, account_id: &str) -> bool {
        // Main patterns for NEAR staking pools
        account_id.ends_with(".poolv1.near") ||
        account_id.ends_with(".pool.near") ||
        account_id.ends_with(".stakingpool") ||
        // Legacy patterns
        account_id.ends_with(".pool.f863973.m0") // TestNet
    }

    /// Parse staking pool function call arguments
    fn parse_staking_args(&self, args: &Vec<u8>) -> Option<serde_json::Value> {
        match serde_json::from_slice::<serde_json::Value>(args) {
            Ok(args_json) => Some(args_json),
            Err(_) => {
                // If JSON parsing fails, try treating as empty args for methods like stake_all, unstake_all
                Some(serde_json::json!({}))
            }
        }
    }

    /// Process delegation actions from receipts
    async fn process_delegation_receipts(
        &self,
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let height = msg.block.header.height as i64;
        let timestamp = msg.block.header.timestamp_nanosec as i64;
        let epoch_id = msg.block.header.epoch_id.to_string();
        let mut delegation_actions = Vec::new();

        // Process receipt execution outcomes
        for shard in &msg.shards {
            for reo in &shard.receipt_execution_outcomes {
                let receipt_id = reo.receipt.receipt_id.to_string();
                let receiver_id = reo.receipt.receiver_id.to_string();

                // Check if this is a staking pool contract
                if !self.is_staking_pool(&receiver_id) {
                    continue;
                }

                // Update known staking pools
                {
                    let mut pools = self.staking_pools.write().await;
                    pools.insert(receiver_id.clone());
                }

                // Process actions in this receipt
                if let ReceiptEnumView::Action { actions, .. } = &reo.receipt.receipt {
                    for action in actions {
                        if let ActionView::FunctionCall {
                            method_name,
                            args,
                            deposit,
                            ..
                        } = action
                        {
                            // Check if this is a staking-related method
                            let is_staking_method = matches!(
                                method_name.as_str(),
                                "deposit"
                                    | "deposit_and_stake"
                                    | "stake"
                                    | "stake_all"
                                    | "unstake"
                                    | "unstake_all"
                                    | "withdraw"
                                    | "withdraw_all"
                            );

                            if is_staking_method {
                                let parsed_args = self.parse_staking_args(args);
                                let amount = if *deposit > 0 {
                                    Some(*deposit)
                                } else {
                                    // Try to extract amount from args for stake/unstake
                                    parsed_args
                                        .as_ref()
                                        .and_then(|args| args.get("amount"))
                                        .and_then(|amt| amt.as_str())
                                        .and_then(|s| s.parse::<u128>().ok())
                                };

                                let success = matches!(reo.execution_outcome.outcome.status,
                                    near_lake_framework::near_indexer_primitives::views::ExecutionStatusView::SuccessValue(_) |
                                    near_lake_framework::near_indexer_primitives::views::ExecutionStatusView::SuccessReceiptId(_)
                                );

                                // Find the transaction hash (from predecessor)
                                let tx_hash =
                                    self.find_transaction_hash_for_receipt(&receipt_id, msg);

                                delegation_actions.push(DelegationAction {
                                    delegator_id: reo.receipt.predecessor_id.to_string(),
                                    pool_id: receiver_id.clone(),
                                    action_type: method_name.clone(),
                                    amount,
                                    args: parsed_args,
                                    transaction_hash: tx_hash,
                                    receipt_id: receipt_id.clone(),
                                    block_height: height,
                                    timestamp,
                                    epoch_id: epoch_id.clone(),
                                    success,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Store delegation actions
        if !delegation_actions.is_empty() {
            self.store_delegation_actions(delegation_actions, db)
                .await?;
        }

        Ok(())
    }

    /// Find transaction hash for a given receipt
    ///
    /// NOTE: This is a simplified implementation that uses receipt_id as a fallback.
    /// A complete implementation would track the relationship between transactions and receipts
    /// by building a mapping from receipt outcomes in the streamer message.
    /// For delegation tracking purposes, using receipt_id is sufficient for uniqueness.
    fn find_transaction_hash_for_receipt(
        &self,
        receipt_id: &str,
        _msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
    ) -> String {
        receipt_id.to_string()
    }

    /// Store delegation actions in the database
    async fn store_delegation_actions(
        &self,
        actions: Vec<DelegationAction>,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let mut action_models = Vec::new();
        let mut delegator_updates = HashMap::new();
        let mut pool_updates = HashMap::new();

        for action in actions {
            // Create delegation action record
            action_models.push(near_delegation_actions::ActiveModel {
                id: NotSet,
                delegator_account_id: Set(action.delegator_id.clone()),
                validator_pool_id: Set(action.pool_id.clone()),
                action_type: Set(action.action_type.clone()),
                amount: Set(action
                    .amount
                    .map(Self::safe_u128_to_decimal)
                    .unwrap_or(Decimal::ZERO)),
                block_height: Set(action.block_height),
                timestamp: Set(action.timestamp),
                epoch_id: Set(action.epoch_id.clone()),
                transaction_hash: Set(action.transaction_hash.clone()),
                receipt_id: Set(action.receipt_id.clone()),
                gas_used: Set(None), // Could be extracted from execution outcome if needed
                success: Set(if action.success { 1 } else { 0 }),
                args_json: Set(action.args),
            });

            // Track for delegator updates
            let delegator_key = (action.delegator_id.clone(), action.pool_id.clone());
            delegator_updates.insert(
                delegator_key,
                (action.block_height, action.timestamp, action.success),
            );

            // Track for pool updates
            pool_updates.insert(
                action.pool_id.clone(),
                (action.block_height, action.timestamp),
            );

            debug!(
                delegator = %action.delegator_id,
                pool = %action.pool_id,
                action = %action.action_type,
                amount = ?action.amount,
                success = action.success,
                "Processed delegation action"
            );
        }

        // Insert delegation actions
        if !action_models.is_empty() {
            near_delegation_actions::Entity::insert_many(action_models)
                .exec(db)
                .await
                .context("Failed to insert delegation actions")?;
        }

        // Update delegator records
        self.update_delegator_records(delegator_updates, db).await?;

        // Update staking pool records
        self.update_staking_pool_records(pool_updates, db).await?;

        Ok(())
    }

    /// Update delegator records with latest activity
    async fn update_delegator_records(
        &self,
        updates: HashMap<(String, String), (i64, i64, bool)>,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        for ((delegator_id, pool_id), (height, timestamp, success)) in updates {
            // Check if delegator record exists
            let existing = near_delegators::Entity::find()
                .filter(near_delegators::Column::DelegatorAccountId.eq(&delegator_id))
                .filter(near_delegators::Column::ValidatorPoolId.eq(&pool_id))
                .one(db)
                .await?;

            if let Some(delegator) = existing {
                // Update existing record
                let mut active: near_delegators::ActiveModel = delegator.into();
                active.last_action_height = Set(Some(height));
                active.last_action_timestamp = Set(Some(timestamp));
                active.is_active = Set(if success { 1 } else { 0 });
                active.update(db).await?;
            } else {
                // Create new delegator record
                let new_delegator = near_delegators::ActiveModel {
                    id: NotSet,
                    delegator_account_id: Set(delegator_id),
                    validator_pool_id: Set(pool_id),
                    first_delegation_height: Set(height),
                    first_delegation_timestamp: Set(timestamp),
                    last_action_height: Set(Some(height)),
                    last_action_timestamp: Set(Some(timestamp)),
                    is_active: Set(if success { 1 } else { 0 }),
                    total_delegated: Set(Decimal::ZERO), // Will be updated via RPC queries
                    total_rewards_earned: Set(Decimal::ZERO),
                };
                near_delegators::Entity::insert(new_delegator)
                    .on_conflict_do_nothing()
                    .exec(db)
                    .await?;
            }
        }
        Ok(())
    }

    /// Update staking pool records
    async fn update_staking_pool_records(
        &self,
        updates: HashMap<String, (i64, i64)>,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        for (pool_id, (height, timestamp)) in updates {
            // Extract validator name from pool ID (e.g., "validator.poolv1.near" -> "validator")
            let validator_name = pool_id.split('.').next().unwrap_or(&pool_id).to_string();
            let factory = if pool_id.ends_with(".poolv1.near") {
                "poolv1.near"
            } else if pool_id.ends_with(".pool.near") {
                "pool.near"
            } else {
                "unknown"
            };

            // Check if staking pool record exists
            let existing = near_staking_pools::Entity::find_by_id(&pool_id)
                .one(db)
                .await?;

            if let Some(pool) = existing {
                // Update existing record
                let mut active: near_staking_pools::ActiveModel = pool.into();
                active.last_seen_height = Set(Some(height));
                active.last_seen_timestamp = Set(Some(timestamp));
                active.update(db).await?;
            } else {
                // Create new staking pool record
                let new_pool = near_staking_pools::ActiveModel {
                    pool_id: Set(pool_id.clone()),
                    validator_account_id: Set(validator_name.clone()),
                    factory_contract: Set(factory.to_string()),
                    owner_id: Set(validator_name), // Default, will be updated via RPC
                    public_key: Set(None),         // Will be populated via RPC queries
                    commission_rate: Set(Decimal::ZERO), // Will be updated via RPC
                    total_staked: Set(Decimal::ZERO), // Will be updated via RPC
                    total_delegators: Set(0),      // Will be calculated
                    first_seen_height: Set(height),
                    first_seen_timestamp: Set(timestamp),
                    last_seen_height: Set(Some(height)),
                    last_seen_timestamp: Set(Some(timestamp)),
                    is_active: Set(1),
                    kickout_reason: Set(None),
                };
                near_staking_pools::Entity::insert(new_pool)
                    .on_conflict_do_nothing()
                    .exec(db)
                    .await?;
            }
        }
        Ok(())
    }

    /// Calculate APY for validators at epoch boundaries
    async fn calculate_validator_apy(
        &self,
        epoch_id: &str,
        epoch_start_height: i64,
        epoch_end_height: i64,
        calculation_timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // Get all active staking pools
        let staking_pools = near_staking_pools::Entity::find()
            .filter(near_staking_pools::Column::IsActive.eq(1))
            .all(db)
            .await?;

        let epochs_per_year = 730; // Approximately 365 days * 2 epochs per day (~12 hours each)
        let mut apy_records = Vec::new();

        for pool in staking_pools {
            // Get validator rewards for this epoch
            let epoch_rewards: Option<rust_decimal::Decimal> =
                near_validator_rewards::Entity::find()
                    .filter(
                        near_validator_rewards::Column::ValidatorId.eq(&pool.validator_account_id),
                    )
                    .filter(near_validator_rewards::Column::EpochId.eq(epoch_id))
                    .all(db)
                    .await?
                    .iter()
                    .map(|r| r.reward_amount)
                    .reduce(|acc, reward| acc + reward);

            let total_rewards = epoch_rewards.unwrap_or(Decimal::ZERO);

            // Skip if no rewards or no stake
            if total_rewards == Decimal::ZERO || pool.total_staked == Decimal::ZERO {
                continue;
            }

            // Calculate epoch reward rate
            let epoch_reward_rate = total_rewards / pool.total_staked;

            // Calculate gross APR and APY
            let gross_apr = epoch_reward_rate * Decimal::from(epochs_per_year) * Decimal::from(100);

            // APY calculation: ((1 + epoch_rate)^epochs_per_year - 1) * 100%
            // Using approximation for compound interest: APY ≈ APR for low rates
            let gross_apy = if epoch_reward_rate < Decimal::new(1, 4) {
                // Less than 0.01%
                gross_apr // Linear approximation for small rates
            } else {
                // More accurate compound calculation for higher rates
                let base = Decimal::ONE + epoch_reward_rate;
                let compound = self.decimal_power(base, epochs_per_year);
                (compound - Decimal::ONE) * Decimal::from(100)
            };

            // Calculate net rates after commission
            let commission_factor = Decimal::ONE - pool.commission_rate;
            let net_apr = gross_apr * commission_factor;
            let net_apy = gross_apy * commission_factor;

            apy_records.push(near_validator_apy::ActiveModel {
                id: NotSet,
                validator_pool_id: Set(pool.pool_id.clone()),
                epoch_id: Set(epoch_id.to_string()),
                epoch_start_height: Set(epoch_start_height),
                epoch_end_height: Set(epoch_end_height),
                total_staked: Set(pool.total_staked),
                total_rewards: Set(total_rewards),
                commission_rate: Set(pool.commission_rate),
                gross_apr: Set(gross_apr),
                gross_apy: Set(gross_apy),
                net_apr: Set(net_apr),
                net_apy: Set(net_apy),
                calculation_timestamp: Set(calculation_timestamp),
                epochs_in_calculation: Set(1), // Single epoch calculation
            });

            debug!(
                pool_id = %pool.pool_id,
                epoch_id = epoch_id,
                total_staked = %pool.total_staked,
                total_rewards = %total_rewards,
                gross_apr = %gross_apr,
                net_apr = %net_apr,
                net_apy = %net_apy,
                "Calculated validator APY"
            );
        }

        // Insert APY records
        if !apy_records.is_empty() {
            let record_count = apy_records.len();
            near_validator_apy::Entity::insert_many(apy_records)
                .exec(db)
                .await
                .context("Failed to insert validator APY records")?;

            info!(
                epoch_id = epoch_id,
                validators_processed = record_count,
                "Calculated APY for validators"
            );
        }

        Ok(())
    }

    /// Simple decimal power approximation for APY calculation
    fn decimal_power(&self, base: Decimal, exponent: i32) -> Decimal {
        if exponent == 0 {
            return Decimal::ONE;
        }

        // For large exponents, use logarithmic approximation to avoid overflow
        if exponent > 1000 {
            // ln(base^exp) = exp * ln(base)
            // For small base values close to 1, ln(1+x) ≈ x
            let base_minus_one = base - Decimal::ONE;
            if base_minus_one.abs() < Decimal::new(1, 2) {
                // Less than 0.01
                return Decimal::ONE + base_minus_one * Decimal::from(exponent);
            }
        }

        // Simple iterative power calculation for reasonable exponents
        let mut result = Decimal::ONE;
        for _ in 0..std::cmp::min(exponent, 100) {
            result *= base;
        }
        result
    }

    async fn process_validator_state_changes(
        &self,
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
        _db: &impl ConnectionTrait,
    ) -> Result<()> {
        for shard in &msg.shards {
            for state_change in &shard.state_changes {
                if matches!(
                    state_change.cause,
                    StateChangeCauseView::ValidatorAccountsUpdate
                ) {
                    debug!(
                        height = msg.block.header.height,
                        "ValidatorAccountsUpdate state change detected"
                    );
                    // We've detected a validator set update, but the actual data
                    // is already captured in the block header proposals
                }
            }
        }
        Ok(())
    }
}

#[async_trait]
impl IndexerModule for ValidatorsModule {
    fn name(&self) -> &str {
        MODULE_NAME
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    async fn process_block(
        &self,
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()> {
        let b = build_block::build_block(msg);
        let height = b.header.height;
        let epoch_id = b.header.epoch_id.clone();
        let timestamp = b.header.timestamp_nanosec as i64;

        debug!(
            height,
            epoch_id, "Processing comprehensive validators data for block"
        );

        let txn = db.begin().await?;

        // 1. Check for epoch transitions
        self.check_epoch_transition(&epoch_id, height, msg, &txn)
            .await
            .context("Failed to check epoch transition")?;

        // 2. Track block production performance
        self.track_block_production(msg.block.author.as_ref(), &epoch_id)
            .await
            .context("Failed to track block production")?;

        // 3. Record block producer (existing functionality)
        self.record_block_producer(
            msg.block.author.as_ref(),
            height,
            &epoch_id,
            timestamp,
            b.header.gas_price,
            &txn,
        )
        .await
        .context("Failed to record block producer")?;

        // 4. Track chunk production and record chunk producers
        for (shard_idx, shard) in msg.shards.iter().enumerate() {
            if let Some(chunk) = &shard.chunk {
                let producer_id = b
                    .chunk_producers
                    .get(shard_idx)
                    .and_then(|cp| cp.producer.as_deref());

                if let Some(producer_id) = producer_id {
                    // Track chunk production performance
                    self.track_chunk_production(producer_id, shard_idx as u64, &epoch_id)
                        .await
                        .context("Failed to track chunk production")?;

                    // Record chunk validator rewards (existing functionality)
                    self.record_chunk_validator_rewards(
                        &chunk.header,
                        Some(producer_id),
                        shard_idx,
                        height,
                        &epoch_id,
                        timestamp,
                        &txn,
                    )
                    .await
                    .context("Failed to record chunk validator rewards")?;
                }
            }
        }

        // Record chunk producers (existing functionality)
        self.record_chunk_producers(msg, &txn)
            .await
            .context("Failed to record chunk producers")?;

        // 5. Record validator proposals from block header
        if !msg.block.header.validator_proposals.is_empty() {
            let context = ProposalContext {
                height,
                epoch_id: epoch_id.clone(),
                timestamp,
                source: "block".to_string(),
                shard_id: None,
            };
            self.record_validator_proposals(&msg.block.header.validator_proposals, &context, &txn)
                .await
                .context("Failed to record block validator proposals")?;
        }

        // 6. Record validator proposals from chunk headers
        for (shard_idx, shard) in msg.shards.iter().enumerate() {
            if let Some(chunk) = &shard.chunk {
                if !chunk.header.validator_proposals.is_empty() {
                    let context = ProposalContext {
                        height,
                        epoch_id: epoch_id.clone(),
                        timestamp,
                        source: "chunk".to_string(),
                        shard_id: Some(shard_idx as i32),
                    };
                    self.record_validator_proposals(
                        &chunk.header.validator_proposals,
                        &context,
                        &txn,
                    )
                    .await
                    .context("Failed to record chunk validator proposals")?;
                }
            }
        }

        // 7. Record validator approvals
        self.record_validator_approvals(
            &msg.block.header.approvals,
            height,
            &epoch_id,
            timestamp,
            &txn,
        )
        .await
        .context("Failed to record validator approvals")?;

        // 8. Record validator rewards for this block
        self.record_validator_rewards(
            msg.block.header.validator_reward,
            msg.block.author.as_ref(),
            height,
            &epoch_id,
            timestamp,
            &txn,
        )
        .await
        .context("Failed to record validator rewards")?;

        // 9. Process slashing events from challenges_result
        if !msg.block.header.challenges_result.is_empty() {
            for slashed_validator in &msg.block.header.challenges_result {
                self.record_slashing_event(
                    slashed_validator.account_id.as_ref(),
                    &epoch_id,
                    height,
                    0u128, // SlashedValidator doesn't contain stake amount
                    "slashing_challenge",
                    timestamp,
                    &txn,
                )
                .await
                .context("Failed to record slashing event")?;
            }
        }

        // 10. Process delegation actions from receipts
        self.process_delegation_receipts(msg, &txn)
            .await
            .context("Failed to process delegation receipts")?;

        // 11. Process validator state changes
        self.process_validator_state_changes(msg, &txn)
            .await
            .context("Failed to process validator state changes")?;

        // 12. Update staking pool data via RPC if configured
        if self
            .rpc_updater_config
            .as_ref()
            .map(|c| c.enabled)
            .unwrap_or(false)
        {
            self.update_staking_pools_via_rpc(height, &txn)
                .await
                .context("Failed to update staking pools via RPC")?;
        }

        txn.commit().await?;

        debug!(
            height = height,
            epoch_id = epoch_id,
            block_producer = msg.block.author.as_str(),
            chunks_count = msg.shards.len(),
            "Successfully processed comprehensive validator data"
        );

        Ok(())
    }

    async fn get_cursor(&self, db: &DatabaseConnection) -> Result<Option<i64>> {
        cursor::get_module_cursor(db, MODULE_NAME).await
    }

    async fn update_cursor(&self, db: &DatabaseConnection, height: i64) -> Result<()> {
        cursor::upsert_module_cursor(db, MODULE_NAME, height).await
    }

    async fn initialize(&self, _db: &DatabaseConnection) -> Result<()> {
        // RPC updates are now integrated inline with block processing for unified cursor-based approach
        if let Some(rpc_config) = &self.rpc_updater_config {
            if rpc_config.enabled {
                info!(
                    rpc_url = %rpc_config.rpc_url,
                    update_interval_minutes = rpc_config.update_interval_minutes,
                    "RPC updater configured for inline processing with archival endpoint"
                );
            }
        }

        Ok(())
    }
}

impl ValidatorsModule {
    /// Update staking pools via RPC for the current block height
    async fn update_staking_pools_via_rpc(
        &self,
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // Get known staking pools from our tracked pools
        let staking_pools = self.staking_pools.read().await;

        if staking_pools.is_empty() {
            debug!(
                block_height,
                "No staking pools tracked yet, skipping RPC update"
            );
            return Ok(());
        }

        let rpc_config = self
            .rpc_updater_config
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("RPC updater config not available"))?;

        // Create RPC updater instance for this block
        let updater = super::rpc_updater::StakingPoolRpcUpdater::with_rate_limit(
            rpc_config.rpc_url.clone(),
            rpc_config.update_interval_minutes,
            10, // Higher rate limit for live processing
        );

        let historical_query = super::rpc_updater::HistoricalQuery {
            block_height: Some(block_height as u64),
            block_hash: None,
        };

        let mut updated_count = 0;
        let mut error_count = 0;

        // Update each known staking pool with data from this block height
        for pool_id in staking_pools.iter() {
            match self
                .update_single_pool_rpc(&updater, pool_id, &historical_query, db)
                .await
            {
                Ok(true) => updated_count += 1,
                Ok(false) => {} // No update needed
                Err(e) => {
                    error_count += 1;
                    debug!(
                        error = %e,
                        pool_id = %pool_id,
                        block_height,
                        "Failed to update staking pool via RPC"
                    );
                }
            }
        }

        if updated_count > 0 || error_count > 0 {
            debug!(
                block_height,
                updated_count,
                error_count,
                total_pools = staking_pools.len(),
                "Completed RPC updates for block"
            );
        }

        Ok(())
    }

    /// Update a single staking pool via RPC
    async fn update_single_pool_rpc(
        &self,
        updater: &super::rpc_updater::StakingPoolRpcUpdater,
        pool_id: &str,
        historical_query: &super::rpc_updater::HistoricalQuery,
        db: &impl ConnectionTrait,
    ) -> Result<bool> {
        // Query historical data for this specific block
        let total_staked = match updater
            .query_total_staked_balance_at(pool_id, Some(historical_query))
            .await
        {
            Ok(balance) => balance,
            Err(e) => {
                debug!(error = %e, pool_id, "Failed to query staked balance, skipping");
                return Ok(false);
            }
        };

        let commission_rate = match updater
            .query_commission_rate_at(pool_id, Some(historical_query))
            .await
        {
            Ok(rate) => rate,
            Err(e) => {
                debug!(error = %e, pool_id, "Failed to query commission rate, skipping");
                return Ok(false);
            }
        };

        // Count current delegators (from our indexed data)
        use sea_orm::PaginatorTrait;
        let delegator_count = near_delegators::Entity::find()
            .filter(near_delegators::Column::ValidatorPoolId.eq(pool_id))
            .filter(near_delegators::Column::IsActive.eq(1))
            .count(db)
            .await
            .context("Failed to count delegators")? as i32;

        // Check if pool record exists, create or update
        let existing_pool = near_staking_pools::Entity::find()
            .filter(near_staking_pools::Column::PoolId.eq(pool_id))
            .one(db)
            .await
            .context("Failed to query existing pool")?;

        let block_height = historical_query.block_height.unwrap_or(0) as i64;
        let timestamp = chrono::Utc::now().timestamp();

        if let Some(pool) = existing_pool {
            // Update existing pool
            let mut active_pool: near_staking_pools::ActiveModel = pool.into();
            active_pool.total_staked = Set(total_staked);
            active_pool.commission_rate = Set(commission_rate);
            active_pool.total_delegators = Set(delegator_count);
            active_pool.last_seen_height = Set(Some(block_height));
            active_pool.last_seen_timestamp = Set(Some(timestamp));

            active_pool
                .update(db)
                .await
                .context("Failed to update existing staking pool")?;
        } else {
            // Create new pool record - need more info for required fields
            debug!(pool_id, "Cannot create new staking pool record from RPC data alone - missing required fields (validator_account_id, factory_contract, etc.)");
            return Ok(false);
        }

        Ok(true)
    }
}
