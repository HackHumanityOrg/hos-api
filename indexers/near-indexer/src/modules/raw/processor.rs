use anyhow::{Context, Result};
use async_trait::async_trait;
use entities::*;
use near_lake_framework::near_indexer_primitives::views::BlockView;
use near_lake_framework::near_indexer_primitives::{IndexerShard, StreamerMessage};
use sea_orm::{ConnectionTrait, DatabaseConnection, EntityTrait, Set, TransactionTrait};
use serde_json;
use std::time::Instant;
use tracing::{info, instrument};

use crate::core::cursor;
use crate::IndexerModule;

const MODULE_NAME: &str = "raw";

pub struct RawModule {
    enabled: bool,
}

impl RawModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        let enabled = match config {
            common::config::AppConfig::Indexer(idx) => {
                let idx = idx.as_ref();
                let near_config = idx.near.as_ref();

                // Check if NEAR indexing is enabled at all
                let near_enabled = near_config.map(|c| c.chain != "disabled").unwrap_or(false);

                if !near_enabled {
                    return Self { enabled: false };
                }

                // Get raw module configuration
                near_config
                    .and_then(|c| c.modules.as_ref())
                    .and_then(|m| m.raw.as_ref())
                    .map(|r| r.enabled)
                    .unwrap_or(true) // Default enabled if not specified
            }
            _ => false,
        };

        Self { enabled }
    }

    async fn store_block(&self, block: &BlockView, db: &impl ConnectionTrait) -> Result<()> {
        let header = &block.header;

        // Store raw block data only
        let block_record = near_blocks::ActiveModel {
            height: Set(header.height as i64),
            hash: Set(header.hash.to_string()),
            prev_hash: Set(header.prev_hash.to_string()),
            epoch_id: Set(header.epoch_id.to_string()),
            next_epoch_id: Set(header.next_epoch_id.to_string()),
            author: Set(block.author.to_string()),
            timestamp: Set(header.timestamp as i64),
            timestamp_nanosec: Set(header.timestamp_nanosec as i64),
            gas_price: Set(header.gas_price.to_string()),
            total_supply: Set(header.total_supply.to_string()),
            validator_reward: Set(header.validator_reward.to_string()),
            protocol_version: Set(header.latest_protocol_version as i32),
            chunks_included: Set(header.chunks_included as i64),
            block_ordinal: Set(header.block_ordinal.map(|bo| bo as i64)),
            prev_height: Set(header.prev_height.map(|ph| ph as i64)),
        };

        near_blocks::Entity::insert(block_record)
            .on_conflict_do_nothing()
            .exec(db)
            .await?;
        Ok(())
    }

    async fn store_chunks(
        &self,
        shards: &[IndexerShard],
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        for shard in shards {
            if let Some(chunk) = &shard.chunk {
                let header = &chunk.header;

                // Store raw chunk data using correct entity structure
                let chunk_record = near_chunks::ActiveModel {
                    chunk_hash: Set(header.chunk_hash.to_string()),
                    block_height: Set(block_height),
                    shard_id: Set(Into::<u64>::into(header.shard_id) as i64),
                    author: Set(chunk.author.to_string()),
                    height_created: Set(header.height_created as i64),
                    height_included: Set(header.height_included as i64),
                    gas_used: Set(header.gas_used as i64),
                    gas_limit: Set(header.gas_limit as i64),
                    validator_reward: Set(header.validator_reward.to_string()),
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

    async fn store_transactions(
        &self,
        shards: &[IndexerShard],
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        for shard in shards {
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    let tx = &tx_with_outcome.transaction;

                    // Store raw transaction data
                    let transaction_record = near_transactions::ActiveModel {
                        hash: Set(tx.hash.to_string()),
                        block_height: Set(block_height),
                        signer_id: Set(tx.signer_id.to_string()),
                        receiver_id: Set(tx.receiver_id.to_string()),
                        nonce: Set(tx.nonce as i64),
                        priority_fee: Set(tx.priority_fee as i64),
                        public_key: Set(tx.public_key.to_string()),
                        signature: Set(tx.signature.to_string()),
                        actions: Set(serde_json::to_value(&tx.actions)
                            .ok()
                            .unwrap_or(serde_json::Value::Null)),
                        raw_data: Set(serde_json::to_value(tx).ok()),
                    };

                    near_transactions::Entity::insert(transaction_record)
                        .on_conflict_do_nothing()
                        .exec(db)
                        .await
                        .context("Failed to insert transaction record")?;
                }
            }
        }

        Ok(())
    }

    async fn store_receipts(
        &self,
        shards: &[IndexerShard],
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        for shard in shards {
            // Store transaction receipts from chunks
            if let Some(chunk) = &shard.chunk {
                // Store receipts from transaction outcomes
                for tx_with_outcome in &chunk.transactions {
                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        let receipt_record = near_receipts::ActiveModel {
                            receipt_id: Set(receipt.receipt_id.to_string()),
                            block_height: Set(block_height),
                            predecessor_id: Set(receipt.predecessor_id.to_string()),
                            receiver_id: Set(receipt.receiver_id.to_string()),
                            receipt: Set(serde_json::to_value(&receipt.receipt)
                                .ok()
                                .unwrap_or(serde_json::Value::Null)),
                            raw_data: Set(serde_json::to_value(receipt).ok()),
                        };

                        near_receipts::Entity::insert(receipt_record)
                            .on_conflict_do_nothing()
                            .exec(db)
                            .await
                            .context("Failed to insert tx receipt record")?;
                    }
                }

                // Store chunk receipts
                for receipt in &chunk.receipts {
                    let receipt_record = near_receipts::ActiveModel {
                        receipt_id: Set(receipt.receipt_id.to_string()),
                        block_height: Set(block_height),
                        predecessor_id: Set(receipt.predecessor_id.to_string()),
                        receiver_id: Set(receipt.receiver_id.to_string()),
                        receipt: Set(serde_json::to_value(&receipt.receipt)
                            .ok()
                            .unwrap_or(serde_json::Value::Null)),
                        raw_data: Set(serde_json::to_value(receipt).ok()),
                    };

                    near_receipts::Entity::insert(receipt_record)
                        .on_conflict_do_nothing()
                        .exec(db)
                        .await
                        .context("Failed to insert chunk receipt record")?;
                }
            }

            // Store receipt execution outcomes receipts
            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;

                let receipt_record = near_receipts::ActiveModel {
                    receipt_id: Set(receipt.receipt_id.to_string()),
                    block_height: Set(block_height),
                    predecessor_id: Set(receipt.predecessor_id.to_string()),
                    receiver_id: Set(receipt.receiver_id.to_string()),
                    receipt: Set(serde_json::to_value(&receipt.receipt)
                        .ok()
                        .unwrap_or(serde_json::Value::Null)),
                    raw_data: Set(serde_json::to_value(receipt).ok()),
                };

                near_receipts::Entity::insert(receipt_record)
                    .on_conflict_do_nothing()
                    .exec(db)
                    .await
                    .context("Failed to insert shard receipt record")?;
            }
        }

        Ok(())
    }

    async fn store_execution_outcomes(
        &self,
        shards: &[IndexerShard],
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        for shard in shards {
            // Store execution outcomes from transaction outcomes
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    let outcome = &tx_with_outcome.outcome.execution_outcome;
                    let execution_outcome_record = near_execution_outcomes::ActiveModel {
                        id: Set(outcome.id.to_string()),
                        block_height: Set(block_height),
                        receipt_id: Set(tx_with_outcome
                            .outcome
                            .receipt
                            .as_ref()
                            .map(|r| r.receipt_id.to_string())),
                        outcome: Set(serde_json::to_value(&outcome.outcome)
                            .ok()
                            .unwrap_or(serde_json::Value::Null)),
                        raw_data: Set(serde_json::to_value(outcome).ok()),
                    };

                    near_execution_outcomes::Entity::insert(execution_outcome_record)
                        .on_conflict_do_nothing()
                        .exec(db)
                        .await
                        .context("Failed to insert tx execution outcome record")?;
                }
            }

            // Store receipt execution outcomes
            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let outcome = &outcome_with_receipt.execution_outcome;
                let receipt = &outcome_with_receipt.receipt;

                let execution_outcome_record = near_execution_outcomes::ActiveModel {
                    id: Set(outcome.id.to_string()),
                    block_height: Set(block_height),
                    receipt_id: Set(Some(receipt.receipt_id.to_string())),
                    outcome: Set(serde_json::to_value(&outcome.outcome)
                        .ok()
                        .unwrap_or(serde_json::Value::Null)),
                    raw_data: Set(serde_json::to_value(outcome).ok()),
                };

                near_execution_outcomes::Entity::insert(execution_outcome_record)
                    .on_conflict_do_nothing()
                    .exec(db)
                    .await
                    .context("Failed to insert shard execution outcome record")?;
            }
        }

        Ok(())
    }

    async fn store_state_changes(
        &self,
        shards: &[IndexerShard],
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        for shard in shards {
            if !shard.state_changes.is_empty() {
                let state_changes_record = near_state_changes::ActiveModel {
                    id: sea_orm::ActiveValue::NotSet,
                    block_height: Set(block_height),
                    shard_id: Set(Into::<u64>::into(shard.shard_id) as i64),
                    state_changes: Set(serde_json::to_value(&shard.state_changes)
                        .ok()
                        .unwrap_or(serde_json::Value::Null)),
                };

                near_state_changes::Entity::insert(state_changes_record)
                    .exec(db)
                    .await
                    .context("Failed to insert state changes record")?;
            }
        }

        Ok(())
    }
}

#[async_trait]
impl IndexerModule for RawModule {
    fn name(&self) -> &str {
        MODULE_NAME
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    async fn initialize(&self, _db: &DatabaseConnection) -> Result<()> {
        // No initialization needed for raw data storage
        Ok(())
    }

    #[instrument(skip(self, msg, db), fields(module = "raw", height = %msg.block.header.height))]
    async fn process_block(&self, msg: &StreamerMessage, db: &DatabaseConnection) -> Result<()> {
        let start_time = Instant::now();
        info!(
            height = msg.block.header.height,
            shard_count = msg.shards.len(),
            "Processing raw block data"
        );

        let txn = db.begin().await?;

        // Store raw block data
        self.store_block(&msg.block, &txn)
            .await
            .context("Failed to store block data")?;

        // Store raw chunk data
        self.store_chunks(&msg.shards, msg.block.header.height as i64, &txn)
            .await
            .context("Failed to store chunk data")?;

        // Store raw transaction data
        self.store_transactions(&msg.shards, msg.block.header.height as i64, &txn)
            .await
            .context("Failed to store transaction data")?;

        // Store raw receipt data
        self.store_receipts(&msg.shards, msg.block.header.height as i64, &txn)
            .await
            .context("Failed to store receipt data")?;

        // Store raw execution outcome data
        self.store_execution_outcomes(&msg.shards, msg.block.header.height as i64, &txn)
            .await
            .context("Failed to store execution outcome data")?;

        // Store raw state change data
        self.store_state_changes(&msg.shards, msg.block.header.height as i64, &txn)
            .await
            .context("Failed to store state change data")?;

        txn.commit().await?;

        let elapsed = start_time.elapsed();
        info!(
            height = msg.block.header.height,
            shard_count = msg.shards.len(),
            elapsed_ms = elapsed.as_millis(),
            "Successfully stored raw block data"
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
