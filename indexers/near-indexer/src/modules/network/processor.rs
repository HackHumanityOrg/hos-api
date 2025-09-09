use anyhow::{Context, Result};
use async_trait::async_trait;
use entities::*;
use rust_decimal::{Decimal, RoundingStrategy};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, Set, TransactionTrait,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::core::{build_block, cursor};
use crate::IndexerModule;

const MODULE_NAME: &str = "network";

pub struct NetworkModule {
    enabled: bool,
    last_epoch_id: Arc<RwLock<Option<String>>>,
    last_gas_price: Arc<RwLock<Option<Decimal>>>,
    last_block_timestamp: Arc<RwLock<Option<u64>>>,
}

impl NetworkModule {
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
                        last_gas_price: Arc::new(RwLock::new(None)),
                        last_block_timestamp: Arc::new(RwLock::new(None)),
                    };
                }

                // Get network module configuration
                let network_config = near_config
                    .and_then(|nc| nc.modules.as_ref())
                    .and_then(|mc| mc.network.as_ref());

                network_config.map(|nc| nc.enabled).unwrap_or(true)
            }
            _ => false,
        };

        Self {
            enabled,
            last_epoch_id: Arc::new(RwLock::new(None)),
            last_gas_price: Arc::new(RwLock::new(None)),
            last_block_timestamp: Arc::new(RwLock::new(None)),
        }
    }

    async fn record_network_state(
        &self,
        header: &crate::core::lake_types::BlockHeaderView,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let network_state = near_network_state::ActiveModel {
            block_height: Set(header.height),
            epoch_id: Set(header.epoch_id.clone()),
            next_epoch_id: Set(header.next_epoch_id.clone()),
            block_hash: Set(header.hash.clone()),
            prev_hash: Set(header.prev_hash.clone()),
            chunks_included: Set(header.chunks_included),
            rent_paid: Set(Self::safe_u128_to_decimal(header.rent_paid)),
            validator_reward: Set(Self::safe_u128_to_decimal(header.validator_reward)),
            last_final_block: Set(header.last_final_block.clone()),
            last_ds_final_block: Set(header.last_ds_final_block.clone()),
            next_bp_hash: Set(header.next_bp_hash.clone()),
            block_merkle_root: Set(header.block_merkle_root.clone()),
            random_value: Set(header.random_value.clone()),
            timestamp: Set(header.timestamp),
            timestamp_nanosec: Set(header.timestamp_nanosec),
            block_ordinal: Set(header.block_ordinal),
            prev_height: Set(header.prev_height),
            gas_price: Set(Self::safe_u128_to_decimal(header.gas_price)),
            protocol_version: Set(header.protocol_version as i32),
            total_supply: Set(Self::safe_u128_to_decimal(header.total_supply)),
        };

        near_network_state::Entity::insert(network_state)
            .on_conflict_do_nothing()
            .exec(db)
            .await?;
        Ok(())
    }

    async fn handle_epoch_transition(
        &self,
        old_epoch_id: &str,
        new_epoch_id: &str,
        height: i64,
        timestamp: u64,
        protocol_version: u32,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        debug!(
            height,
            old_epoch = old_epoch_id,
            new_epoch = new_epoch_id,
            "Epoch transition detected"
        );

        // Update the end of the previous epoch
        if let Some(existing_epoch) = near_epochs::Entity::find()
            .filter(near_epochs::Column::EpochId.eq(old_epoch_id))
            .one(db)
            .await?
        {
            let mut active: near_epochs::ActiveModel = existing_epoch.into();
            active.end_height = Set(Some(height - 1));
            active.end_timestamp = Set(Some(timestamp as i64));
            active.update(db).await?;
        }

        // Start the new epoch
        let new_epoch = near_epochs::ActiveModel {
            epoch_id: Set(new_epoch_id.to_string()),
            start_height: Set(height),
            start_timestamp: Set(timestamp as i64),
            end_height: Set(None),
            end_timestamp: Set(None),
            protocol_version: Set(protocol_version as i32),
        };

        near_epochs::Entity::insert(new_epoch)
            .on_conflict_do_nothing()
            .exec(db)
            .await?;
        Ok(())
    }

    async fn check_epoch_transition(
        &self,
        current_epoch_id: &str,
        height: i64,
        timestamp: u64,
        protocol_version: u32,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let mut last_epoch = self.last_epoch_id.write().await;

        match last_epoch.as_ref() {
            Some(previous_epoch) if previous_epoch != current_epoch_id => {
                // Epoch transition detected
                self.handle_epoch_transition(
                    previous_epoch,
                    current_epoch_id,
                    height,
                    timestamp,
                    protocol_version,
                    db,
                )
                .await?;
            }
            None => {
                // First epoch we're seeing, just record it
                let new_epoch = near_epochs::ActiveModel {
                    epoch_id: Set(current_epoch_id.to_string()),
                    start_height: Set(height),
                    start_timestamp: Set(timestamp as i64),
                    end_height: Set(None),
                    end_timestamp: Set(None),
                    protocol_version: Set(protocol_version as i32),
                };

                near_epochs::Entity::insert(new_epoch)
            .on_conflict_do_nothing()
            .exec(db)
            .await?;
                debug!(height, epoch_id = current_epoch_id, "First epoch recorded");
            }
            _ => {
                // Same epoch, no transition
            }
        }

        *last_epoch = Some(current_epoch_id.to_string());
        Ok(())
    }

    async fn record_chunk_data(
        &self,
        chunk_producers: &[crate::core::lake_types::ChunkProducer],
        header: &crate::core::lake_types::BlockHeaderView,
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        debug!(
            height = header.height,
            chunk_count = chunk_producers.len(),
            "Recording chunk data"
        );

        for (idx, chunk_producer) in chunk_producers.iter().enumerate() {
            // Find corresponding shard data
            let shard = msg.shards.get(idx).context("Shard index out of bounds")?;
            
            let (gas_used, gas_limit, tx_count, receipt_count, validator_reward) = 
                if let Some(chunk) = &shard.chunk {
                    let gas_used = chunk.header.gas_used;
                    let gas_limit = chunk.header.gas_limit;
                    let tx_count = chunk.transactions.len() as i32;
                    let receipt_count = shard.receipt_execution_outcomes.len() as i32;
                    let validator_reward = chunk.header.validator_reward;
                    (gas_used, gas_limit, tx_count, receipt_count, validator_reward)
                } else {
                    (0, 0, 0, 0, 0)
                };

            if let Some(chunk_hash) = &chunk_producer.chunk_hash {
                let chunk_record = near_chunks::ActiveModel {
                    id: sea_orm::ActiveValue::NotSet,
                    chunk_hash: Set(chunk_hash.clone()),
                    block_height: Set(header.height),
                    shard_id: Set(chunk_producer.shard_id as i64),
                    chunk_producer: Set(chunk_producer.producer.clone()),
                    gas_used: Set(gas_used as i64),
                    gas_limit: Set(gas_limit as i64),
                    transactions_count: Set(tx_count),
                    receipts_count: Set(receipt_count),
                    timestamp: Set(header.timestamp_nanosec as i64),
                    validator_reward: Set(Self::safe_u128_to_decimal(validator_reward)),
                };

                near_chunks::Entity::insert(chunk_record)
                    .on_conflict_do_nothing()
                    .exec(db)
                    .await
                    .context("Failed to insert chunk record")?;
            }
        }

        Ok(())
    }

    async fn calculate_and_record_gas_price_dynamics(
        &self,
        header: &crate::core::lake_types::BlockHeaderView,
        _chunk_producers: &[crate::core::lake_types::ChunkProducer],
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // NEAR gas price minimum floor: 100,000,000 yoctoNEAR per gas (0.0001 NEAR per TGas)
        const MIN_GAS_PRICE: u128 = 100_000_000;
        let raw_gas_price = header.gas_price.max(MIN_GAS_PRICE);
        let current_gas_price = Self::safe_u128_to_decimal(raw_gas_price);
        
        // Calculate total gas used/limit across all shards
        let mut total_gas_used = 0u64;
        let mut total_gas_limit = 0u64;
        
        // In NEAR, each shard has a gas limit of 1 PetaGas (10^15) regardless of whether chunks are present
        const SHARD_GAS_LIMIT: u64 = 1_000_000_000_000_000; // 1 PetaGas
        
        for shard in &msg.shards {
            if let Some(chunk) = &shard.chunk {
                total_gas_used += chunk.header.gas_used;
                total_gas_limit += chunk.header.gas_limit;
            } else {
                // Missing chunk: gas_used = 0, but gas_limit still applies
                total_gas_limit += SHARD_GAS_LIMIT;
            }
        }

        // Get previous gas price
        let mut prev_gas_price_guard = self.last_gas_price.write().await;
        let prev_gas_price = prev_gas_price_guard.clone();

        // Calculate utilization ratio and price adjustment
        let (utilization_ratio, price_adjustment_percent) = if total_gas_limit > 0 {
            let ratio = Decimal::from(total_gas_used) / Decimal::from(total_gas_limit);
            // NEAR gas price adjustment formula: 1% adjustment based on utilization vs 50%
            let adjustment = if let Some(_prev_price) = &prev_gas_price {
                let target_utilization = Decimal::from_str_exact("0.5").unwrap(); // 50%
                let adjustment_factor = (ratio - target_utilization) * Decimal::from_str_exact("0.01").unwrap(); // 1% (0.01 factor)
                Some(adjustment_factor.round_dp_with_strategy(2, RoundingStrategy::MidpointNearestEven))
            } else {
                None
            };
            (Some(ratio.round_dp_with_strategy(6, RoundingStrategy::MidpointNearestEven)), adjustment)
        } else {
            (None, None)
        };

        let gas_price_record = near_gas_prices::ActiveModel {
            block_height: Set(header.height),
            gas_price: Set(current_gas_price.clone()),
            prev_gas_price: Set(prev_gas_price.clone()),
            gas_used: Set(total_gas_used as i64),
            gas_limit: Set(total_gas_limit as i64),
            utilization_ratio: Set(utilization_ratio),
            price_adjustment_percent: Set(price_adjustment_percent),
            timestamp: Set(header.timestamp_nanosec as i64),
        };

        near_gas_prices::Entity::insert(gas_price_record)
            .on_conflict_do_nothing()
            .exec(db)
            .await
            .context("Failed to insert gas price record")?;

        // Update last gas price
        *prev_gas_price_guard = Some(current_gas_price);

        debug!(
            height = header.height,
            gas_price = %raw_gas_price,
            original_gas_price = %header.gas_price,
            gas_used = total_gas_used,
            gas_limit = total_gas_limit,
            utilization = ?utilization_ratio,
            "Recorded gas price dynamics"
        );

        Ok(())
    }

    async fn record_block_metrics(
        &self,
        header: &crate::core::lake_types::BlockHeaderView,
        chunk_producers: &[crate::core::lake_types::ChunkProducer],
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // Calculate block timing
        let mut block_time_ms_guard = self.last_block_timestamp.write().await;
        let block_time_ms = if let Some(last_timestamp) = *block_time_ms_guard {
            if header.timestamp_nanosec > last_timestamp {
                Some((header.timestamp_nanosec - last_timestamp) / 1_000_000) // Convert to ms
            } else {
                Some(0) // Block time is 0 or negative, likely out-of-order processing
            }
        } else {
            None
        };
        *block_time_ms_guard = Some(header.timestamp_nanosec);

        // Count chunks produced vs missing
        let chunks_produced = chunk_producers.iter()
            .filter(|cp| cp.chunk_hash.is_some() && cp.producer.is_some())
            .count() as i32;
        let total_shards = msg.shards.len() as i32;
        let chunks_missing = total_shards - chunks_produced;

        // Calculate totals across all shards
        let mut total_gas_used = 0i64;
        let mut total_gas_limit = 0i64;
        let mut total_transactions = 0i32;

        for shard in &msg.shards {
            if let Some(chunk) = &shard.chunk {
                total_gas_used += chunk.header.gas_used as i64;
                total_gas_limit += chunk.header.gas_limit as i64;
                total_transactions += chunk.transactions.len() as i32;
            }
        }

        // Calculate epoch progress (approximate based on blocks since epoch start)
        let epoch_progress_percent = if let Some(prev_height) = header.prev_height {
            // Epoch is typically 43200 blocks
            let blocks_in_epoch = header.height - (prev_height as i64);
            let estimated_progress = (blocks_in_epoch % 43200) as f64 / 43200.0 * 100.0;
            Some(Decimal::from_f64_retain(estimated_progress)
                .unwrap_or(Decimal::ZERO)
                .round_dp_with_strategy(2, RoundingStrategy::MidpointNearestEven))
        } else {
            None
        };

        // Extract approvals count from block header
        let approvals_count = msg.block.header.approvals.iter()
            .filter(|approval| approval.is_some())
            .count() as i32;

        // Extract challenges count from block header
        let challenges_count = msg.block.header.challenges_result.len() as i32;

        let block_metrics = near_block_metrics::ActiveModel {
            block_height: Set(header.height),
            block_time_ms: Set(block_time_ms.map(|t| t as i64)),
            chunks_produced: Set(chunks_produced),
            chunks_missing: Set(chunks_missing),
            total_gas_used: Set(total_gas_used),
            total_gas_limit: Set(total_gas_limit),
            total_transactions: Set(total_transactions),
            approvals_count: Set(Some(approvals_count)),
            challenges_count: Set(challenges_count),
            epoch_progress_percent: Set(epoch_progress_percent),
            timestamp: Set(header.timestamp_nanosec as i64),
        };

        near_block_metrics::Entity::insert(block_metrics)
            .on_conflict_do_nothing()
            .exec(db)
            .await
            .context("Failed to insert block metrics")?;

        debug!(
            height = header.height,
            block_time_ms = ?block_time_ms,
            chunks_produced = chunks_produced,
            chunks_missing = chunks_missing,
            total_transactions = total_transactions,
            approvals_count = approvals_count,
            challenges_count = challenges_count,
            "Recorded block metrics"
        );

        Ok(())
    }

    async fn initialize_from_db(&self, db: &DatabaseConnection) -> Result<()> {
        // Load the last seen epoch from database when starting up
        if let Some(last_epoch) = near_epochs::Entity::find()
            .order_by_desc(near_epochs::Column::StartHeight)
            .one(db)
            .await?
        {
            let epoch_id = last_epoch.epoch_id.clone();
            let mut last_epoch_id = self.last_epoch_id.write().await;
            *last_epoch_id = Some(last_epoch.epoch_id);
            debug!(
                epoch_id = %epoch_id,
                "Initialized with last epoch from DB"
            );
        }

        // Load last gas price
        if let Some(last_gas_price) = near_gas_prices::Entity::find()
            .order_by_desc(near_gas_prices::Column::BlockHeight)
            .one(db)
            .await?
        {
            let mut gas_price_guard = self.last_gas_price.write().await;
            *gas_price_guard = Some(last_gas_price.gas_price);
            debug!(
                gas_price = %last_gas_price.gas_price,
                "Initialized with last gas price from DB"
            );
        }

        // Load last block timestamp
        if let Some(last_metrics) = near_block_metrics::Entity::find()
            .order_by_desc(near_block_metrics::Column::BlockHeight)
            .one(db)
            .await?
        {
            let mut timestamp_guard = self.last_block_timestamp.write().await;
            *timestamp_guard = Some(last_metrics.timestamp as u64);
            debug!(
                timestamp = last_metrics.timestamp,
                "Initialized with last block timestamp from DB"
            );
        }

        Ok(())
    }
}

#[async_trait]
impl IndexerModule for NetworkModule {
    fn name(&self) -> &str {
        MODULE_NAME
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    async fn initialize(&self, db: &DatabaseConnection) -> Result<()> {
        self.initialize_from_db(db).await
    }

    async fn process_block(
        &self,
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()> {
        let b = build_block::build_block(msg);
        let header = &b.header;

        info!(
            height = header.height,
            epoch_id = header.epoch_id,
            shards = msg.shards.len(),
            chunks = b.chunk_producers.len(),
            "Processing comprehensive network state for block"
        );

        let txn = db.begin().await?;

        // 1. Check for epoch transitions
        self.check_epoch_transition(
            &header.epoch_id,
            header.height,
            header.timestamp_nanosec,
            header.protocol_version,
            &txn,
        )
        .await
        .context("Failed to check epoch transition")?;

        // 2. Record basic network state
        self.record_network_state(header, &txn)
            .await
            .context("Failed to record network state")?;

        // 3. Record chunk data (shard-level metrics)
        self.record_chunk_data(&b.chunk_producers, header, msg, &txn)
            .await
            .context("Failed to record chunk data")?;

        // 4. Calculate and record gas price dynamics
        self.calculate_and_record_gas_price_dynamics(header, &b.chunk_producers, msg, &txn)
            .await
            .context("Failed to record gas price dynamics")?;

        // 5. Record comprehensive block metrics
        self.record_block_metrics(header, &b.chunk_producers, msg, &txn)
            .await
            .context("Failed to record block metrics")?;

        txn.commit().await?;

        debug!(
            height = header.height,
            gas_price = %header.gas_price,
            total_supply = %header.total_supply,
            "Successfully processed network state"
        );

        Ok(())
    }

    async fn get_cursor(&self, db: &DatabaseConnection) -> Result<Option<i64>> {
        cursor::get_module_cursor(db, MODULE_NAME).await
    }

    async fn update_cursor(&self, db: &DatabaseConnection, height: i64) -> Result<()> {
        cursor::upsert_module_cursor(db, MODULE_NAME, height).await
    }
}
