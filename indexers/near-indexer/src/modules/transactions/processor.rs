use anyhow::Result;
use entities;
use near_lake_framework::near_indexer_primitives::{views::ExecutionStatusView, StreamerMessage};
use rust_decimal::Decimal;
use sea_orm::DatabaseConnection;
use sea_orm::{sea_query::OnConflict, EntityTrait, Set, TransactionTrait};
use serde_json::json;
use tracing::{debug, info};

use crate::core::cursor;
use crate::IndexerModule;

const MODULE_NAME: &str = "transactions";

pub struct TransactionsProcessor {
    enabled: bool,
}

impl TransactionsProcessor {
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }

    async fn process_transactions(
        &self,
        msg: &StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()> {
        let height = msg.block.header.height as i64;
        let ts = msg.block.header.timestamp_nanosec as i64;
        let block_hash = msg.block.header.hash.to_string();

        let txn = db.begin().await?;
        let mut transaction_inserts = Vec::new();

        for shard in &msg.shards {
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    let tx = &tx_with_outcome.transaction;
                    let outcome = &tx_with_outcome.outcome.execution_outcome;

                    // Serialize actions to JSON
                    let actions_json = json!(tx.actions).to_string();

                    // Extract receipt IDs
                    let receipt_ids_json = json!(outcome.outcome.receipt_ids).to_string();

                    // Determine transaction status
                    let status = match &outcome.outcome.status {
                        ExecutionStatusView::SuccessValue(_) => "success",
                        ExecutionStatusView::SuccessReceiptId(_) => "success",
                        ExecutionStatusView::Failure(_) => "failure",
                        ExecutionStatusView::Unknown => "unknown",
                    };

                    // Convert gas and tokens to appropriate types
                    let gas_used = outcome.outcome.gas_burnt as i64;
                    let tokens_burnt =
                        Decimal::from_str_exact(&outcome.outcome.tokens_burnt.to_string())
                            .unwrap_or(Decimal::ZERO);

                    let tx_record = entities::near_transactions::ActiveModel {
                        tx_hash: Set(tx.hash.to_string()),
                        signer_id: Set(tx.signer_id.to_string()),
                        receiver_id: Set(tx.receiver_id.to_string()),
                        block_height: Set(height),
                        block_time: Set(ts),
                        block_hash: Set(block_hash.clone()),
                        actions: Set(Some(actions_json)),
                        status: Set(status.to_string()),
                        receipt_ids: Set(Some(receipt_ids_json)),
                        gas_used: Set(Some(gas_used)),
                        tokens_burnt: Set(Some(tokens_burnt)),
                    };
                    transaction_inserts.push(tx_record);

                    debug!(
                        tx_hash = %tx.hash,
                        signer = %tx.signer_id,
                        receiver = %tx.receiver_id,
                        status = status,
                        gas_used = gas_used,
                        "processed transaction"
                    );
                }
            }
        }

        let tx_count = transaction_inserts.len();
        if !transaction_inserts.is_empty() {
            entities::near_transactions::Entity::insert_many(transaction_inserts)
                .on_conflict(
                    OnConflict::column(entities::near_transactions::Column::TxHash)
                        .do_nothing()
                        .to_owned(),
                )
                .exec(&txn)
                .await?;
        }

        txn.commit().await?;
        info!(
            height,
            module = MODULE_NAME,
            transactions_processed = tx_count,
            "processed block"
        );
        Ok(())
    }
}

#[async_trait::async_trait]
impl IndexerModule for TransactionsProcessor {
    fn name(&self) -> &str {
        MODULE_NAME
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    async fn process_block(&self, msg: &StreamerMessage, db: &DatabaseConnection) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }
        self.process_transactions(msg, db).await
    }

    async fn get_cursor(&self, db: &DatabaseConnection) -> Result<Option<i64>> {
        cursor::get_module_cursor(db, MODULE_NAME).await
    }

    async fn update_cursor(&self, db: &DatabaseConnection, height: i64) -> Result<()> {
        // Only advance cursor when moving forward
        let current = self.get_cursor(db).await?;
        if current.map(|h| height > h).unwrap_or(true) {
            cursor::upsert_module_cursor(db, MODULE_NAME, height).await?;
        }
        Ok(())
    }

    async fn initialize(&self, _db: &DatabaseConnection) -> Result<()> {
        info!(module = MODULE_NAME, enabled = self.enabled, "initialized");
        Ok(())
    }
}
