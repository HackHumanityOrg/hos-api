use anyhow::{Context, Result};
use async_trait::async_trait;
use entities::*;
use near_indexer_primitives::views::BlockView;
use near_indexer_primitives::{IndexerShard, StreamerMessage};
use near_primitives::views::ActionView;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Set, TransactionTrait,
};
use tracing::{debug, info};

use crate::core::cursor;
use crate::modules::staking_pools::{
    action_parser::ActionParser,
    event_parser::EventParser,
    pool_detector::PoolDetector,
    types::{ActionContext, DetectionMethod, PoolType, StakingAction},
};
use crate::IndexerModule;

const MODULE_NAME: &str = "staking_pools";

pub struct StakingPoolsModule {
    enabled: bool,
    pool_detector: PoolDetector,
    track_epochs: bool,
    track_delegators: bool,
    parse_events: bool,
}

impl StakingPoolsModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        let (enabled, pool_patterns, custom_pools, track_epochs, track_delegators, parse_events) = 
            match config {
                common::config::AppConfig::Indexer(idx) => {
                    let near_config = idx.near.as_ref();

                    // Check if NEAR indexing is enabled at all
                    let near_enabled = near_config.map(|c| c.chain != "disabled").unwrap_or(false);

                    if !near_enabled {
                        return Self {
                            enabled: false,
                            pool_detector: PoolDetector::new(vec![], vec![]),
                            track_epochs: false,
                            track_delegators: false,
                            parse_events: false,
                        };
                    }

                    // Get staking pools module configuration
                    let staking_config = near_config
                        .and_then(|c| c.modules.as_ref())
                        .and_then(|m| m.staking_pools.as_ref());

                    let enabled = staking_config.map(|s| s.enabled).unwrap_or(false);
                    let pool_patterns = staking_config
                        .map(|s| s.pool_patterns.clone())
                        .unwrap_or_else(|| vec!["*.poolv1.near".to_string(), "*.pool.near".to_string()]);
                    let custom_pools = staking_config
                        .map(|s| s.custom_pools.clone())
                        .unwrap_or_default();
                    let track_epochs = staking_config.map(|s| s.track_epochs).unwrap_or(true);
                    let track_delegators = staking_config.map(|s| s.track_delegators).unwrap_or(true);
                    let parse_events = staking_config.map(|s| s.parse_events).unwrap_or(true);

                    (enabled, pool_patterns, custom_pools, track_epochs, track_delegators, parse_events)
                }
                _ => (false, vec![], vec![], false, false, false),
            };

        let pool_detector = PoolDetector::new(pool_patterns, custom_pools);

        Self {
            enabled,
            pool_detector,
            track_epochs,
            track_delegators,
            parse_events,
        }
    }

    /// Track epoch transitions from block headers
    async fn process_epoch_transitions(&self, block: &BlockView, db: &impl ConnectionTrait) -> Result<()> {
        if !self.track_epochs {
            return Ok(());
        }

        let header = &block.header;
        let epoch_id = header.epoch_id.to_string();
        let next_epoch_id = header.next_epoch_id.to_string();

        // Check if this is a new epoch
        let existing_epoch = near_staking_epochs::Entity::find_by_id(&epoch_id)
            .one(db)
            .await?;

        if existing_epoch.is_none() {
            // New epoch started - note: epoch_height would need to be computed from block info
            let epoch_record = near_staking_epochs::ActiveModel {
                epoch_id: Set(epoch_id.clone()),
                epoch_height: Set(0), // This would need proper calculation from chain data
                start_block_height: Set(header.height as i64),
                end_block_height: Set(None),
                protocol_version: Set(Some(header.latest_protocol_version as i32)),
                total_stake: Set("0".to_string()),
                total_validator_reward: Set("0".to_string()),
                validators_info: Set(None), // Could be populated from block validator proposals
                chunk_producers_settlement: Set(None),
                epoch_duration_blocks: Set(None),
            };

            near_staking_epochs::Entity::insert(epoch_record)
                .on_conflict_do_nothing()
                .exec(db)
                .await?;

            debug!("New epoch detected: {} starting at block {}", epoch_id, header.height);
        }

        // Update rewards for current epoch
        if let Some(epoch) = existing_epoch {
            // Add current block reward to epoch total
            let current_reward = header.validator_reward.to_string().parse::<u128>().unwrap_or(0);
            let current_total: u128 = epoch.total_validator_reward.parse().unwrap_or(0);
            let new_total = current_total + current_reward;

            let mut epoch: near_staking_epochs::ActiveModel = epoch.into();
            epoch.total_validator_reward = Set(new_total.to_string());
            epoch.update(db).await?;
        }

        // Check if previous epoch ended
        if epoch_id != next_epoch_id {
            // This block transitions to a new epoch
            self.finalize_previous_epochs(header.height as i64, db).await?;
        }

        Ok(())
    }

    /// Finalize previous epochs that haven't been closed
    async fn finalize_previous_epochs(&self, current_height: i64, db: &impl ConnectionTrait) -> Result<()> {
        // Find epochs that don't have an end height set
        let open_epochs = near_staking_epochs::Entity::find()
            .filter(near_staking_epochs::Column::EndBlockHeight.is_null())
            .filter(near_staking_epochs::Column::StartBlockHeight.lt(current_height))
            .all(db)
            .await?;

        for epoch in open_epochs {
            let epoch_id = epoch.epoch_id.clone();
            let mut epoch: near_staking_epochs::ActiveModel = epoch.into();
            epoch.end_block_height = Set(Some(current_height - 1));
            let duration = current_height - 1 - *epoch.start_block_height.as_ref();
            epoch.epoch_duration_blocks = Set(Some(duration));
            epoch.update(db).await?;

            debug!("Finalized epoch: {} ended at block {}", epoch_id, current_height - 1);
        }

        Ok(())
    }

    /// Process staking-related actions from transactions and receipts
    async fn process_staking_actions(&self, shard: &IndexerShard, db: &impl ConnectionTrait) -> Result<()> {
        let mut new_pools = Vec::new();
        let mut staking_actions = Vec::new();

        // Process transactions
        if let Some(chunk) = &shard.chunk {
            for tx_with_outcome in &chunk.transactions {
                let tx = &tx_with_outcome.transaction;
                let outcome = &tx_with_outcome.outcome.execution_outcome;

                let context = ActionContext {
                    predecessor_id: tx.signer_id.clone(),
                    receiver_id: tx.receiver_id.clone(),
                    transaction_hash: tx.hash.to_string(),
                    receipt_id: outcome.id.to_string(),
                    block_height: shard.chunk.as_ref().unwrap().header.height_included,
                    timestamp: shard.chunk.as_ref().unwrap().header.height_created as i64,
                };

                // Check if receiver is a known pool
                let is_known_pool = self.pool_detector.detect_pool(&context.receiver_id.to_string()).is_some();

                // Parse actions
                let actions = ActionParser::parse_actions(&tx.actions, &context, is_known_pool);
                
                // Check for new pool detection
                if !is_known_pool {
                    if let Some(detection) = self.check_for_new_pool(&tx.actions, &context.receiver_id.to_string()) {
                        new_pools.push((context.receiver_id.clone(), detection.0, detection.1));
                    }
                }

                staking_actions.extend(actions);
            }
        }

        // Process receipts
        for receipt_with_outcome in &shard.receipt_execution_outcomes {
            let receipt = &receipt_with_outcome.receipt;
            let outcome = &receipt_with_outcome.execution_outcome;

            let context = ActionContext {
                predecessor_id: receipt.predecessor_id.clone(),
                receiver_id: receipt.receiver_id.clone(),
                transaction_hash: "".to_string(), // Receipts don't have tx hash directly
                receipt_id: receipt.receipt_id.to_string(),
                block_height: shard.chunk.as_ref().unwrap().header.height_included,
                timestamp: shard.chunk.as_ref().unwrap().header.height_created as i64,
            };

            // Only process Action receipts
            if let Some(raw_actions) = self.extract_actions_from_receipt(&receipt) {
                let is_known_pool = self.pool_detector.detect_pool(&context.receiver_id.to_string()).is_some();

                // Check for new pool detection before parsing
                if !is_known_pool {
                    if let Some(detection) = self.check_for_new_pool(&raw_actions, &context.receiver_id.to_string()) {
                        new_pools.push((context.receiver_id.clone(), detection.0, detection.1));
                    }
                }

                let actions = ActionParser::parse_actions(&raw_actions, &context, is_known_pool);

                staking_actions.extend(actions);

                // Parse events if enabled
                if self.parse_events && (is_known_pool || new_pools.iter().any(|(pool_id, _, _)| pool_id == &context.receiver_id)) {
                    let events = EventParser::parse_events(
                        &outcome.outcome.logs,
                        &context.receiver_id.to_string(),
                        &outcome.id.to_string(),
                        context.block_height,
                        context.timestamp,
                    );

                    for event in events {
                        self.store_event(event, db).await?;
                    }
                }
            }
        }

        // Store new pools
        for (pool_id, pool_type, detection_method) in new_pools {
            self.register_new_pool(&pool_id.to_string(), pool_type, detection_method, 
                                 shard.chunk.as_ref().unwrap().header.height_included as i64, db).await?;
        }

        // Store staking actions
        for action in staking_actions {
            self.store_staking_action(action, db).await?;
        }

        Ok(())
    }

    /// Extract actions from a receipt (simplified)
    fn extract_actions_from_receipt(&self, receipt: &near_indexer_primitives::views::ReceiptView) -> Option<Vec<ActionView>> {
        match &receipt.receipt {
            near_indexer_primitives::views::ReceiptEnumView::Action { actions, .. } => Some(actions.clone()),
            _ => None,
        }
    }

    /// Check if actions indicate a new staking pool
    fn check_for_new_pool(&self, actions: &[ActionView], receiver_id: &str) -> Option<(PoolType, DetectionMethod)> {
        if self.pool_detector.could_be_pool_by_method(receiver_id) {
            // Check if any action is a staking method
            for action in actions {
                if let ActionView::FunctionCall { method_name, .. } = action {
                    if crate::modules::staking_pools::types::is_staking_method(method_name) {
                        return Some((PoolType::Custom, DetectionMethod::MethodCall));
                    }
                }
            }
        }
        None
    }

    /// Register a new staking pool
    async fn register_new_pool(
        &self,
        pool_id: &str,
        pool_type: PoolType,
        detection_method: DetectionMethod,
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let pool_record = near_staking_pools::ActiveModel {
            pool_id: Set(pool_id.to_string()),
            pool_type: Set(pool_type.as_str().to_string()),
            detected_method: Set(detection_method.as_str().to_string()),
            first_seen_height: Set(block_height),
            last_activity_height: Set(block_height),
            is_active: Set(1), // MySQL boolean as tinyint
            metadata: Set(None),
        };

        near_staking_pools::Entity::insert(pool_record)
            .on_conflict_do_nothing()
            .exec(db)
            .await?;

        info!("Registered new staking pool: {} (type: {}, detected by: {})", 
              pool_id, pool_type.as_str(), detection_method.as_str());

        Ok(())
    }

    /// Store a staking action in the database
    async fn store_staking_action(&self, action: StakingAction, db: &impl ConnectionTrait) -> Result<()> {
        match action {
            StakingAction::PoolCall { pool_id, method_name, caller_account_id, args_base64, args_json, 
                                    attached_deposit, transaction_hash, receipt_id, block_height, timestamp, success } => {
                let pool_action_record = near_staking_pool_actions::ActiveModel {
                    id: Set(Default::default()), // Auto-increment
                    pool_id: Set(pool_id.to_string()),
                    method_name: Set(method_name.clone()),
                    caller_account_id: Set(caller_account_id.to_string()),
                    args_json: Set(args_json.map(Into::into)),
                    args_base64: Set(args_base64),
                    attached_deposit: Set(attached_deposit.to_string()),
                    transaction_hash: Set(transaction_hash),
                    receipt_id: Set(receipt_id),
                    block_height: Set(block_height as i64),
                    timestamp: Set(timestamp),
                    success: Set(success.map(|b| if b { 1 } else { 0 })), // MySQL boolean as tinyint
                };

                near_staking_pool_actions::Entity::insert(pool_action_record)
                    .on_conflict_do_nothing()
                    .exec(db)
                    .await?;

                // Update delegator tracking if enabled
                if self.track_delegators {
                    self.update_delegator(&pool_id.to_string(), &caller_account_id.to_string(), &method_name, block_height as i64, db).await?;
                }

                // Update pool last activity
                self.update_pool_activity(&pool_id.to_string(), block_height as i64, db).await?;
            }
        }

        Ok(())
    }

    /// Store an event
    async fn store_event(&self, event: crate::modules::staking_pools::types::StakingEvent, db: &impl ConnectionTrait) -> Result<()> {
        let event_record = near_staking_pool_events::ActiveModel {
            id: Set(Default::default()), // Auto-increment
            pool_id: Set(event.pool_id.to_string()),
            event_standard: Set(event.event_standard),
            event_type: Set(event.event_type),
            event_data: Set(event.event_data.into()),
            raw_log: Set(event.raw_log),
            log_index: Set(event.log_index),
            execution_outcome_id: Set(event.execution_outcome_id),
            block_height: Set(event.block_height as i64),
            timestamp: Set(event.timestamp),
        };

        near_staking_pool_events::Entity::insert(event_record)
            .on_conflict_do_nothing()
            .exec(db)
            .await?;

        Ok(())
    }

    /// Update delegator relationship
    async fn update_delegator(
        &self,
        pool_id: &str,
        delegator_id: &str,
        interaction_type: &str,
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // Check if delegator relationship exists
        let existing = near_staking_delegators::Entity::find()
            .filter(near_staking_delegators::Column::PoolId.eq(pool_id))
            .filter(near_staking_delegators::Column::DelegatorAccountId.eq(delegator_id))
            .one(db)
            .await?;

        if let Some(delegator) = existing {
            // Update existing relationship
            let mut delegator: near_staking_delegators::ActiveModel = delegator.into();
            delegator.last_interaction_height = Set(block_height);
            delegator.last_interaction_type = Set(interaction_type.to_string());
            delegator.interaction_count = Set(delegator.interaction_count.as_ref() + 1);
            delegator.update(db).await?;
        } else {
            // Create new relationship
            let delegator_record = near_staking_delegators::ActiveModel {
                id: Set(Default::default()), // Auto-increment
                pool_id: Set(pool_id.to_string()),
                delegator_account_id: Set(delegator_id.to_string()),
                first_interaction_height: Set(block_height),
                first_interaction_type: Set(interaction_type.to_string()),
                last_interaction_height: Set(block_height),
                last_interaction_type: Set(interaction_type.to_string()),
                interaction_count: Set(1),
            };

            near_staking_delegators::Entity::insert(delegator_record)
                .on_conflict_do_nothing()
                .exec(db)
                .await?;
        }

        Ok(())
    }

    /// Update pool last activity
    async fn update_pool_activity(&self, pool_id: &str, block_height: i64, db: &impl ConnectionTrait) -> Result<()> {
        if let Some(pool) = near_staking_pools::Entity::find_by_id(pool_id).one(db).await? {
            let mut pool: near_staking_pools::ActiveModel = pool.into();
            pool.last_activity_height = Set(block_height);
            pool.update(db).await?;
        }

        Ok(())
    }
}

#[async_trait]
impl IndexerModule for StakingPoolsModule {
    fn name(&self) -> &'static str {
        MODULE_NAME
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    async fn process_block(
        &self,
        streamer_message: &StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let block_height = streamer_message.block.header.height;
        debug!("Processing staking pools data for block {}", block_height);

        // Start transaction for atomicity
        let txn = db.begin().await?;

        // Track epoch transitions
        self.process_epoch_transitions(&streamer_message.block, &txn).await
            .with_context(|| format!("Failed to process epoch transitions for block {}", block_height))?;

        // Process each shard
        for shard in &streamer_message.shards {
            self.process_staking_actions(shard, &txn).await
                .with_context(|| format!("Failed to process staking actions for shard {} block {}", 
                                        shard.shard_id, block_height))?;
        }

        txn.commit().await?;

        debug!("Completed staking pools processing for block {}", block_height);
        Ok(())
    }

    async fn get_cursor(&self, db: &DatabaseConnection) -> Result<Option<i64>> {
        cursor::get_module_cursor(db, MODULE_NAME).await
    }

    async fn update_cursor(&self, db: &DatabaseConnection, height: i64) -> Result<()> {
        cursor::upsert_module_cursor(db, MODULE_NAME, height).await
    }
}