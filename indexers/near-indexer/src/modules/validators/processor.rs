use anyhow::Result;
use async_trait::async_trait;
use entities::*;
use near_lake_framework::near_indexer_primitives::views::StateChangeCauseView;
use near_primitives::views::{validator_stake_view::ValidatorStakeView, ChunkHeaderView};
use rust_decimal::Decimal;
use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter, Set, TransactionTrait,
};
use tracing::{debug, warn};

use crate::core::{build_block, cursor};
use crate::IndexerModule;

const MODULE_NAME: &str = "validators";

pub struct ValidatorsModule {
    enabled: bool,
}

#[derive(Debug, Clone)]
struct ProposalContext {
    height: i64,
    epoch_id: String,
    timestamp: i64,
    source: String,
    shard_id: Option<i32>,
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
                    return Self { enabled: false };
                }

                // Get validators module configuration
                let validators_config = near_config
                    .and_then(|nc| nc.modules.as_ref())
                    .and_then(|mc| mc.validators.as_ref());

                validators_config.map(|vc| vc.enabled).unwrap_or(true)
            }
            _ => false,
        };

        Self { enabled }
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

    // TODO: Epoch info functionality will be implemented when we have access to the necessary data structures
    // For now, we focus on the available data from StreamerMessage
    //
    // async fn record_epoch_info(...) { ... }
    // async fn record_block_validators(...) { ... }

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

        debug!(height, epoch_id, "Processing validators for block");

        let txn = db.begin().await?;

        // Record block producer
        self.record_block_producer(
            msg.block.author.as_ref(),
            height,
            &epoch_id,
            timestamp,
            b.header.gas_price,
            &txn,
        )
        .await?;

        // Record chunk producers
        self.record_chunk_producers(msg, &txn).await?;

        // Record validator proposals from block header
        if !msg.block.header.validator_proposals.is_empty() {
            let context = ProposalContext {
                height,
                epoch_id: epoch_id.clone(),
                timestamp,
                source: "block".to_string(),
                shard_id: None,
            };
            self.record_validator_proposals(&msg.block.header.validator_proposals, &context, &txn)
                .await?;
        }

        // Record validator proposals from chunk headers
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
                    .await?;
                }
            }
        }

        // Record validator approvals
        self.record_validator_approvals(
            &msg.block.header.approvals,
            height,
            &epoch_id,
            timestamp,
            &txn,
        )
        .await?;

        // Record validator rewards for this block
        self.record_validator_rewards(
            msg.block.header.validator_reward,
            msg.block.author.as_ref(), // Pass the block producer
            height,
            &epoch_id,
            timestamp,
            &txn,
        )
        .await?;

        // Record chunk validator rewards
        let b = build_block::build_block(msg);
        for (shard_idx, shard) in msg.shards.iter().enumerate() {
            if let Some(chunk) = &shard.chunk {
                let producer_id = b
                    .chunk_producers
                    .get(shard_idx)
                    .and_then(|cp| cp.producer.as_deref());

                self.record_chunk_validator_rewards(
                    &chunk.header,
                    producer_id,
                    shard_idx,
                    height,
                    &epoch_id,
                    timestamp,
                    &txn,
                )
                .await?;
            }
        }

        // TODO: Record epoch info and block validators when data structures are available

        // Process validator state changes
        self.process_validator_state_changes(msg, &txn).await?;

        txn.commit().await?;

        Ok(())
    }

    async fn get_cursor(&self, db: &DatabaseConnection) -> Result<Option<i64>> {
        cursor::get_module_cursor(db, MODULE_NAME).await
    }

    async fn update_cursor(&self, db: &DatabaseConnection, height: i64) -> Result<()> {
        cursor::upsert_module_cursor(db, MODULE_NAME, height).await
    }
}
