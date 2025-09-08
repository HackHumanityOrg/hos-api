use anyhow::Result;
use async_trait::async_trait;
use entities::*;
use rust_decimal::Decimal;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, Set, TransactionTrait,
};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use crate::core::{build_block, cursor};
use crate::IndexerModule;

const MODULE_NAME: &str = "network";

pub struct NetworkModule {
    enabled: bool,
    last_epoch_id: Arc<RwLock<Option<String>>>,
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

        debug!(
            height = header.height,
            epoch_id = header.epoch_id,
            "Processing network state for block"
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
        .await?;

        // 2. Record network state
        self.record_network_state(header, &txn).await?;

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
