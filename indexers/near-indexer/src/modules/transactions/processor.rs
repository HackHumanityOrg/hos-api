use anyhow::Result;
use entities;
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::DatabaseConnection;
use sea_orm::{sea_query::OnConflict, EntityTrait, Set, TransactionTrait};
use tracing::{debug, info};

use crate::core::{build_block, cursor};
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

        let block = build_block::build_block(msg);
        for tx in block.txs {
            let status = match tx.status {
                crate::core::lake_types::ExecStatus::Success => "success",
                crate::core::lake_types::ExecStatus::Failure => "failure",
                crate::core::lake_types::ExecStatus::Unknown => "unknown",
            };
            let receipt_ids_json = serde_json::to_string(&tx.receipt_ids).unwrap_or("[]".into());
            let tx_record = entities::near_transactions::ActiveModel {
                tx_hash: Set(tx.hash.clone()),
                signer_id: Set(tx.signer_id.clone()),
                receiver_id: Set(tx.receiver_id.clone()),
                block_height: Set(height),
                block_time: Set(ts),
                block_hash: Set(block_hash.clone()),
                actions: Set(Some(tx.actions_json.clone())),
                status: Set(status.to_string()),
                receipt_ids: Set(Some(receipt_ids_json)),
                public_key: Set(tx.public_key.clone()),
                nonce: Set(tx.nonce),
                gas_used: Set(Some(tx.gas_burnt)),
                tokens_burnt: Set(Some(tx.tokens_burnt)),
            };
            transaction_inserts.push(tx_record);
            debug!(
                tx_hash = %tx.hash,
                signer = %tx.signer_id,
                receiver = %tx.receiver_id,
                status = status,
                gas_used = tx.gas_burnt,
                "processed transaction"
            );
        }

        let tx_count = transaction_inserts.len();
        if !transaction_inserts.is_empty() {
            use entities::near_transactions::Column as TXC;
            entities::near_transactions::Entity::insert_many(transaction_inserts)
                .on_conflict(
                    OnConflict::column(TXC::TxHash)
                        // No-op style update to satisfy MySQL
                        .update_columns([TXC::TxHash])
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
