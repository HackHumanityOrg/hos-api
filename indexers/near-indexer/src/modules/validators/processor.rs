use anyhow::Result;
use async_trait::async_trait;
use entities::*;
use near_lake_framework::near_indexer_primitives::views::StateChangeCauseView;
use near_primitives::views::validator_stake_view::ValidatorStakeView;
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

        block_producer.insert(db).await?;
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
        let timestamp = b.header.timestamp_ns;

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
                .exec(db)
                .await?;
        }
        Ok(())
    }

    async fn record_validator_proposals(
        &self,
        proposals: &[ValidatorStakeView],
        height: i64,
        epoch_id: &str,
        timestamp: i64,
        source: &str,
        shard_id: Option<i32>,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let mut proposal_models = Vec::new();

        for proposal in proposals {
            let account_id = proposal.clone().take_account_id().to_string();
            let (public_key, stake) = match proposal {
                ValidatorStakeView::V1(v1) => (v1.public_key.to_string(), v1.stake),
            };

            // Insert/update validator record
            self.upsert_validator(&account_id, &public_key, height, timestamp, db)
                .await?;

            // Insert proposal record
            proposal_models.push(near_validator_proposals::ActiveModel {
                id: NotSet,
                block_height: Set(height),
                account_id: Set(account_id),
                public_key: Set(public_key),
                stake: Set(Self::safe_u128_to_decimal(stake)),
                epoch_id: Set(epoch_id.to_string()),
                timestamp: Set(timestamp),
                source: Set(source.to_string()),
                shard_id: Set(shard_id),
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
            new_validator.insert(db).await?;
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

        approval_record.insert(db).await?;
        Ok(())
    }

    async fn process_validator_kickouts(
        &self,
        challenges_result: &[near_primitives::challenge::SlashedValidator],
        height: i64,
        epoch_id: &str,
        timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        if challenges_result.is_empty() {
            return Ok(());
        }

        let mut kickout_models = Vec::new();

        for slashed_validator in challenges_result {
            // Extract validator account id and reason
            let validator_id = slashed_validator.account_id.to_string();
            let reason = match &slashed_validator.is_double_sign {
                true => "double_sign".to_string(),
                false => "other".to_string(), // Could be refined based on actual SlashedValidator fields
            };

            // Record the kickout
            kickout_models.push(entities::near_validator_kickouts::ActiveModel {
                id: NotSet,
                validator_id: Set(validator_id.clone()),
                epoch_id: Set(epoch_id.to_string()),
                block_height: Set(height),
                reason: Set(reason),
                stake_returned: Set(Decimal::ZERO),
                timestamp: Set(timestamp),
            });

            debug!(
                validator_id = %validator_id,
                epoch_id = %epoch_id,
                "Validator kickout recorded"
            );
        }

        if !kickout_models.is_empty() {
            entities::near_validator_kickouts::Entity::insert_many(kickout_models)
                .exec(db)
                .await?;
        }

        Ok(())
    }

    async fn update_validator_seat_prices(
        &self,
        validator_proposals: &[near_primitives::views::validator_stake_view::ValidatorStakeView],
        height: i64,
        epoch_id: &str,
        timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        if validator_proposals.is_empty() {
            return Ok(());
        }

        // Calculate total stake and validator count
        let mut stakes: Vec<u128> = validator_proposals
            .iter()
            .map(|v| match v {
                ValidatorStakeView::V1(v1) => v1.stake,
            })
            .collect();

        // Sort stakes in ascending order to find the seat price (typically the lowest stake that gets a seat)
        stakes.sort();

        let total_stake: u128 = stakes.iter().sum();
        let validator_count = stakes.len() as i32;

        // The seat price is typically the minimum stake needed to become a validator
        // In NEAR, this is usually determined by the number of available seats vs proposals
        let seat_price = stakes.first().copied().unwrap_or(0);

        // Check if we already have a record for this epoch
        let existing = entities::near_validator_seat_prices::Entity::find()
            .filter(entities::near_validator_seat_prices::Column::EpochId.eq(epoch_id))
            .one(db)
            .await?;

        if existing.is_none() {
            let seat_price_model = entities::near_validator_seat_prices::ActiveModel {
                epoch_id: Set(epoch_id.to_string()),
                seat_price: Set(Self::safe_u128_to_decimal(seat_price)),
                total_stake: Set(Self::safe_u128_to_decimal(total_stake)),
                validator_count: Set(validator_count),
                start_height: Set(height),
                start_timestamp: Set(timestamp),
            };

            seat_price_model.insert(db).await?;

            debug!(
                epoch_id = %epoch_id,
                seat_price = %seat_price,
                validator_count = validator_count,
                "Validator seat prices recorded"
            );
        }

        Ok(())
    }

    async fn record_validator_rewards(
        &self,
        validator_reward: u128,
        height: i64,
        epoch_id: &str,
        timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        if validator_reward == 0 {
            return Ok(());
        }

        // Store the total validator reward for this block
        // Using special validator_id "*total*" to indicate this is the aggregate reward for all validators
        let reward_model = entities::near_validator_rewards::ActiveModel {
            id: NotSet,
            validator_id: Set("*total*".to_string()),
            epoch_id: Set(epoch_id.to_string()),
            reward_amount: Set(Self::safe_u128_to_decimal(validator_reward)),
            delegator_rewards: Set(rust_decimal::Decimal::ZERO), // Not available at block level
            commission_earned: Set(rust_decimal::Decimal::ZERO), // Not available at block level
            commission_rate: Set(rust_decimal::Decimal::ZERO),   // Not available at block level
            timestamp: Set(timestamp),
        };

        reward_model.insert(db).await?;

        debug!(
            height = height,
            epoch_id = %epoch_id,
            validator_reward = %validator_reward,
            "Total validator reward recorded for block"
        );

        Ok(())
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
        let timestamp = b.header.timestamp_ns;

        debug!(height, epoch_id, "Processing validators for block");

        let txn = db.begin().await?;

        // 1. Record block producer
        self.record_block_producer(
            msg.block.author.as_ref(),
            height,
            &epoch_id,
            timestamp,
            b.header.gas_price,
            &txn,
        )
        .await?;

        // 2. Record chunk producers
        self.record_chunk_producers(msg, &txn).await?;

        // 3. Record validator proposals from block header
        if !msg.block.header.validator_proposals.is_empty() {
            self.record_validator_proposals(
                &msg.block.header.validator_proposals,
                height,
                &epoch_id,
                timestamp,
                "block",
                None,
                &txn,
            )
            .await?;
        }

        // 4. Record validator proposals from chunk headers
        for (shard_idx, shard) in msg.shards.iter().enumerate() {
            if let Some(chunk) = &shard.chunk {
                if !chunk.header.validator_proposals.is_empty() {
                    self.record_validator_proposals(
                        &chunk.header.validator_proposals,
                        height,
                        &epoch_id,
                        timestamp,
                        "chunk",
                        Some(shard_idx as i32),
                        &txn,
                    )
                    .await?;
                }
            }
        }

        // 5. Record validator approvals
        self.record_validator_approvals(
            &msg.block.header.approvals,
            height,
            &epoch_id,
            timestamp,
            &txn,
        )
        .await?;

        // 6. Process validator kickouts from challenges_result
        self.process_validator_kickouts(
            &msg.block.header.challenges_result,
            height,
            &epoch_id,
            timestamp,
            &txn,
        )
        .await?;

        // 7. Calculate validator seat price for this epoch
        self.update_validator_seat_prices(
            &msg.block.header.validator_proposals,
            height,
            &epoch_id,
            timestamp,
            &txn,
        )
        .await?;

        // 8. Record validator rewards for this block
        self.record_validator_rewards(
            msg.block.header.validator_reward,
            height,
            &epoch_id,
            timestamp,
            &txn,
        )
        .await?;

        // 9. Process validator state changes
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
