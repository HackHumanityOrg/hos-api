use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use common::config::NearModuleStreamMode;
use entities::near::*;
use near_lake_framework::near_indexer_primitives::views::{
    ActionView, BlockView, ExecutionStatusView, ReceiptEnumView,
};
use near_lake_framework::near_indexer_primitives::{IndexerShard, StreamerMessage};
use sea_orm::{
    sea_query::OnConflict, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, Set,
    Statement, TransactionTrait,
};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tracing::{error, info, instrument};

use crate::core::cursor;
use crate::core::receipt_tx_hash_map::{load_receipt_tx_hash_map, upsert_receipt_tx_hash};
use crate::core::receipt_tx_resolver::build_receipt_tx_hashes;
use crate::IndexerModule;

const MODULE_NAME: &str = "raw";

pub struct RawModule {
    enabled: bool,
    stream_mode: NearModuleStreamMode,
}

impl RawModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        let mut enabled = false;
        let mut stream_mode = NearModuleStreamMode::Shared;

        if let common::config::AppConfig::Indexer(idx) = config {
            let idx = idx.as_ref();
            if let Some(near) = idx.near.as_ref() {
                let near_enabled = near.chain != "disabled";
                if near_enabled {
                    enabled = near.raw_enabled_or_default();
                    stream_mode = near.raw_stream_mode_or_default();
                }
            }
        }

        Self {
            enabled,
            stream_mode,
        }
    }

    fn serialize_json<T: serde::Serialize>(value: &T, label: &str) -> Result<JsonValue> {
        serde_json::to_value(value).with_context(|| format!("Failed to serialize {label}"))
    }

    fn parse_args_json(args: &[u8]) -> JsonValue {
        if args.is_empty() {
            return JsonValue::Null;
        }

        serde_json::from_slice::<JsonValue>(args)
            .or_else(|_| {
                String::from_utf8(args.to_vec())
                    .ok()
                    .and_then(|text| serde_json::from_str::<JsonValue>(&text).ok())
                    .ok_or_else(|| serde_json::Error::io(std::io::Error::other("invalid json")))
            })
            .unwrap_or_else(|_| serde_json::json!({ "value": "invalid json" }))
    }

    fn action_kind_from_json(action_json: &JsonValue) -> String {
        match action_json {
            JsonValue::Object(map) => map
                .keys()
                .next()
                .cloned()
                .unwrap_or_else(|| "Unknown".to_string()),
            JsonValue::String(value) => value.clone(),
            _ => "Unknown".to_string(),
        }
    }

    fn extract_status(status: &ExecutionStatusView) -> (&'static str, Option<JsonValue>) {
        match status {
            ExecutionStatusView::Unknown => (
                "Unknown",
                Some(serde_json::json!({"status_type": "Unknown"})),
            ),
            ExecutionStatusView::Failure(failure) => (
                "Failure",
                Some(serde_json::json!({
                    "error": failure.to_string(),
                    "status_type": "Failure",
                })),
            ),
            ExecutionStatusView::SuccessReceiptId(receipt_id) => (
                "SuccessReceiptId",
                Some(serde_json::json!({
                    "receipt_id": receipt_id.to_string(),
                    "status_type": "SuccessReceiptId",
                })),
            ),
            ExecutionStatusView::SuccessValue(raw_value) => {
                if raw_value.is_empty() {
                    return ("SuccessValue", None);
                }

                let parsed = serde_json::from_slice::<JsonValue>(raw_value)
                    .or_else(|_| {
                        String::from_utf8(raw_value.clone())
                            .ok()
                            .and_then(|text| serde_json::from_str::<JsonValue>(&text).ok())
                            .ok_or_else(|| {
                                serde_json::Error::io(std::io::Error::other("invalid json"))
                            })
                    })
                    .unwrap_or_else(|_| serde_json::json!({ "value": "invalid json" }));

                ("SuccessValue", Some(parsed))
            }
        }
    }

    fn shard_to_i64(shard_id: near_lake_framework::near_indexer_primitives::types::ShardId) -> i64 {
        let value: u64 = shard_id.into();
        i64::try_from(value).unwrap_or(i64::MAX)
    }

    async fn persist_receipt_tx_mapping(
        db: &impl ConnectionTrait,
        receipt_id: &str,
        transaction_hash: &str,
        block_height: i64,
    ) -> Result<()> {
        let persisted_tx_hash =
            upsert_receipt_tx_hash(db, receipt_id, transaction_hash, block_height).await?;
        if persisted_tx_hash != transaction_hash {
            error!(
                module = MODULE_NAME,
                receipt_id = %receipt_id,
                incoming_transaction_hash = %transaction_hash,
                persisted_transaction_hash = %persisted_tx_hash,
                "receipt->tx hash conflict detected; preserving first-seen mapping"
            );
        }

        Ok(())
    }

    async fn load_receipt_tx_hashes_from_execution_outcomes(
        db: &impl ConnectionTrait,
        receipt_ids: &[String],
    ) -> Result<HashMap<String, String>> {
        if receipt_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut placeholders = String::new();
        for idx in 0..receipt_ids.len() {
            if idx > 0 {
                placeholders.push_str(", ");
            }
            placeholders.push_str(&format!("${}", idx + 1));
        }

        let sql = format!(
            "SELECT receipt_id, transaction_hash
             FROM near.near_execution_outcomes
             WHERE receipt_id IN ({placeholders})
               AND transaction_hash IS NOT NULL"
        );

        let rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &sql,
                receipt_ids
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect::<Vec<_>>(),
            ))
            .await
            .context("failed loading transaction hashes from near_execution_outcomes")?;

        let mut resolved = HashMap::new();
        for row in rows {
            let receipt_id: String = row
                .try_get("", "receipt_id")
                .context("failed reading near_execution_outcomes.receipt_id")?;
            let transaction_hash: String = row
                .try_get("", "transaction_hash")
                .context("failed reading near_execution_outcomes.transaction_hash")?;
            resolved.insert(receipt_id, transaction_hash);
        }

        Ok(resolved)
    }

    async fn load_receipt_tx_hashes_from_receipt_actions(
        db: &impl ConnectionTrait,
        receipt_ids: &[String],
    ) -> Result<HashMap<String, String>> {
        if receipt_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut placeholders = String::new();
        for idx in 0..receipt_ids.len() {
            if idx > 0 {
                placeholders.push_str(", ");
            }
            placeholders.push_str(&format!("${}", idx + 1));
        }

        let sql = format!(
            "SELECT receipt_id, transaction_hash
             FROM near.near_receipt_actions
             WHERE receipt_id IN ({placeholders})
               AND transaction_hash IS NOT NULL"
        );

        let rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &sql,
                receipt_ids
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect::<Vec<_>>(),
            ))
            .await
            .context("failed loading transaction hashes from near_receipt_actions")?;

        let mut resolved = HashMap::new();
        for row in rows {
            let receipt_id: String = row
                .try_get("", "receipt_id")
                .context("failed reading near_receipt_actions.receipt_id")?;
            let transaction_hash: String = row
                .try_get("", "transaction_hash")
                .context("failed reading near_receipt_actions.transaction_hash")?;
            resolved.insert(receipt_id, transaction_hash);
        }

        Ok(resolved)
    }

    async fn load_receipt_tx_hashes_from_receipt_edges(
        db: &impl ConnectionTrait,
        receipt_ids: &[String],
    ) -> Result<HashMap<String, String>> {
        if receipt_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut placeholders = String::new();
        for idx in 0..receipt_ids.len() {
            if idx > 0 {
                placeholders.push_str(", ");
            }
            placeholders.push_str(&format!("${}", idx + 1));
        }

        let sql = format!(
            "SELECT child_receipt_id AS receipt_id, transaction_hash
             FROM near.near_receipt_edges
             WHERE child_receipt_id IN ({placeholders})
               AND transaction_hash IS NOT NULL
             UNION ALL
             SELECT parent_receipt_id AS receipt_id, transaction_hash
             FROM near.near_receipt_edges
             WHERE parent_receipt_id IN ({placeholders})
               AND transaction_hash IS NOT NULL"
        );

        let mut values = receipt_ids
            .iter()
            .cloned()
            .map(Into::into)
            .collect::<Vec<_>>();
        values.extend(receipt_ids.iter().cloned().map(Into::into));

        let rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &sql,
                values,
            ))
            .await
            .context("failed loading transaction hashes from near_receipt_edges")?;

        let mut resolved = HashMap::new();
        for row in rows {
            let receipt_id: String = row
                .try_get("", "receipt_id")
                .context("failed reading near_receipt_edges.receipt_id")?;
            let transaction_hash: String = row
                .try_get("", "transaction_hash")
                .context("failed reading near_receipt_edges.transaction_hash")?;
            resolved.entry(receipt_id).or_insert(transaction_hash);
        }

        Ok(resolved)
    }

    async fn load_receipt_tx_hashes_from_parent_outcomes(
        db: &impl ConnectionTrait,
        receipt_ids: &[String],
    ) -> Result<HashMap<String, String>> {
        if receipt_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let mut placeholders = String::new();
        for idx in 0..receipt_ids.len() {
            if idx > 0 {
                placeholders.push_str(", ");
            }
            placeholders.push_str(&format!("${}", idx + 1));
        }

        let sql = format!(
            "SELECT re.child_receipt_id AS receipt_id, eo.transaction_hash
             FROM near.near_receipt_edges re
             INNER JOIN near.near_execution_outcomes eo
                ON eo.id = re.parent_execution_outcome_id
             WHERE re.child_receipt_id IN ({placeholders})
               AND eo.transaction_hash IS NOT NULL"
        );

        let rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &sql,
                receipt_ids
                    .iter()
                    .cloned()
                    .map(Into::into)
                    .collect::<Vec<_>>(),
            ))
            .await
            .context("failed loading transaction hashes from parent execution outcomes")?;

        let mut resolved = HashMap::new();
        for row in rows {
            let receipt_id: String = row
                .try_get("", "receipt_id")
                .context("failed reading parent-outcome receipt_id")?;
            let transaction_hash: String = row
                .try_get("", "transaction_hash")
                .context("failed reading parent-outcome transaction_hash")?;
            resolved.entry(receipt_id).or_insert(transaction_hash);
        }

        Ok(resolved)
    }

    async fn resolve_receipt_tx_hashes_from_canonical_graph(
        db: &impl ConnectionTrait,
        receipt_ids: &[String],
    ) -> Result<HashMap<String, String>> {
        let mut unresolved = receipt_ids.to_vec();
        let mut resolved = HashMap::new();

        let apply_stage = |resolved: &mut HashMap<String, String>,
                           unresolved: &mut Vec<String>,
                           stage_rows: HashMap<String, String>| {
            for (receipt_id, tx_hash) in stage_rows {
                resolved.entry(receipt_id).or_insert(tx_hash);
            }

            if !resolved.is_empty() {
                let unresolved_set: HashSet<String> = unresolved.drain(..).collect();
                *unresolved = unresolved_set
                    .into_iter()
                    .filter(|receipt_id| !resolved.contains_key(receipt_id))
                    .collect();
            }
        };

        if !unresolved.is_empty() {
            let stage_rows =
                Self::load_receipt_tx_hashes_from_execution_outcomes(db, &unresolved).await?;
            apply_stage(&mut resolved, &mut unresolved, stage_rows);
        }
        if !unresolved.is_empty() {
            let stage_rows =
                Self::load_receipt_tx_hashes_from_receipt_actions(db, &unresolved).await?;
            apply_stage(&mut resolved, &mut unresolved, stage_rows);
        }
        if !unresolved.is_empty() {
            let stage_rows =
                Self::load_receipt_tx_hashes_from_receipt_edges(db, &unresolved).await?;
            apply_stage(&mut resolved, &mut unresolved, stage_rows);
        }
        if !unresolved.is_empty() {
            let stage_rows =
                Self::load_receipt_tx_hashes_from_parent_outcomes(db, &unresolved).await?;
            apply_stage(&mut resolved, &mut unresolved, stage_rows);
        }

        Ok(resolved)
    }

    fn collect_candidate_receipt_ids(shards: &[IndexerShard]) -> Vec<String> {
        let mut ids = Vec::new();
        for shard in shards {
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        ids.push(receipt.receipt_id.to_string());
                    }
                    for child_receipt_id in &tx_with_outcome
                        .outcome
                        .execution_outcome
                        .outcome
                        .receipt_ids
                    {
                        ids.push(child_receipt_id.to_string());
                    }
                }

                for receipt in &chunk.receipts {
                    ids.push(receipt.receipt_id.to_string());
                }
                for receipt in &chunk.local_receipts {
                    ids.push(receipt.receipt_id.to_string());
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                ids.push(outcome_with_receipt.receipt.receipt_id.to_string());
                for child_receipt_id in &outcome_with_receipt.execution_outcome.outcome.receipt_ids
                {
                    ids.push(child_receipt_id.to_string());
                }
            }
        }
        ids
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_near_execution_outcome(
        &self,
        db: &impl ConnectionTrait,
        receipt_id: Option<&str>,
        execution_outcome_id: &str,
        transaction_hash: Option<&str>,
        block_height: i64,
        block_hash: &str,
        chunk_hash: &str,
        shard_id: i64,
        gas_burnt: i64,
        gas_used: f64,
        tokens_burnt: f64,
        executor_account_id: &str,
        status: &str,
        outcome_receipt_ids: JsonValue,
        logs: JsonValue,
        results_json: Option<JsonValue>,
        outcome_json: JsonValue,
        raw_data_json: JsonValue,
        block_timestamp: i64,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_execution_outcomes (
                id,
                block_height,
                shard_id,
                receipt_id,
                outcome,
                raw_data,
                transaction_hash,
                block_hash,
                chunk_hash,
                gas_burnt,
                gas_used,
                tokens_burnt,
                executor_account_id,
                status,
                outcome_receipt_ids,
                executed_in_block_hash,
                logs,
                results_json,
                block_timestamp
            )
            VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15,
                $16, $17, $18, $19
            )
            ON CONFLICT (id) DO UPDATE SET
                block_height = EXCLUDED.block_height,
                shard_id = EXCLUDED.shard_id,
                receipt_id = COALESCE(near.near_execution_outcomes.receipt_id, EXCLUDED.receipt_id),
                outcome = EXCLUDED.outcome,
                raw_data = COALESCE(near.near_execution_outcomes.raw_data, EXCLUDED.raw_data),
                transaction_hash = COALESCE(near.near_execution_outcomes.transaction_hash, EXCLUDED.transaction_hash),
                block_hash = EXCLUDED.block_hash,
                chunk_hash = EXCLUDED.chunk_hash,
                gas_burnt = EXCLUDED.gas_burnt,
                gas_used = EXCLUDED.gas_used,
                tokens_burnt = EXCLUDED.tokens_burnt,
                executor_account_id = EXCLUDED.executor_account_id,
                status = EXCLUDED.status,
                outcome_receipt_ids = EXCLUDED.outcome_receipt_ids,
                executed_in_block_hash = EXCLUDED.executed_in_block_hash,
                logs = EXCLUDED.logs,
                results_json = EXCLUDED.results_json,
                block_timestamp = EXCLUDED.block_timestamp",
            vec![
                execution_outcome_id.into(),
                block_height.into(),
                shard_id.into(),
                receipt_id.map(ToString::to_string).into(),
                outcome_json.into(),
                raw_data_json.into(),
                transaction_hash.map(ToString::to_string).into(),
                block_hash.into(),
                chunk_hash.into(),
                gas_burnt.into(),
                gas_used.into(),
                tokens_burnt.into(),
                executor_account_id.into(),
                status.into(),
                outcome_receipt_ids.into(),
                block_hash.into(),
                logs.into(),
                results_json.into(),
                block_timestamp.into(),
            ],
        ))
        .await
        .context("Failed to upsert near.near_execution_outcomes")?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_near_receipt_edge(
        &self,
        db: &impl ConnectionTrait,
        parent_receipt_id: &str,
        child_receipt_id: &str,
        parent_execution_outcome_id: &str,
        block_height: i64,
        block_hash: &str,
        block_timestamp: i64,
        transaction_hash: Option<&str>,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_receipt_edges (
                parent_receipt_id,
                child_receipt_id,
                parent_execution_outcome_id,
                block_height,
                block_hash,
                block_timestamp,
                transaction_hash
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (parent_receipt_id, child_receipt_id) DO UPDATE SET
                parent_execution_outcome_id = EXCLUDED.parent_execution_outcome_id,
                block_height = EXCLUDED.block_height,
                block_hash = EXCLUDED.block_hash,
                block_timestamp = EXCLUDED.block_timestamp,
                transaction_hash = COALESCE(
                    near.near_receipt_edges.transaction_hash,
                    EXCLUDED.transaction_hash
                )",
            vec![
                parent_receipt_id.into(),
                child_receipt_id.into(),
                parent_execution_outcome_id.into(),
                block_height.into(),
                block_hash.into(),
                block_timestamp.into(),
                transaction_hash.map(ToString::to_string).into(),
            ],
        ))
        .await
        .context("Failed to upsert near.near_receipt_edges")?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_near_receipt_action(
        &self,
        db: &impl ConnectionTrait,
        id: &str,
        block_height: i64,
        receipt_id: &str,
        execution_outcome_id: &str,
        transaction_hash: Option<&str>,
        shard_id: i64,
        signer_account_id: Option<&str>,
        signer_public_key: Option<&str>,
        gas_price: Option<&str>,
        predecessor_id: &str,
        receiver_id: &str,
        block_hash: &str,
        chunk_hash: &str,
        author: &str,
        action_kind: &str,
        action_json: JsonValue,
        method_name: Option<&str>,
        gas: Option<i64>,
        deposit: Option<&str>,
        args_base64: Option<&str>,
        args_json: Option<JsonValue>,
        action_index: i32,
        block_timestamp: i64,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_receipt_actions (
                id,
                block_height,
                receipt_id,
                execution_outcome_id,
                transaction_hash,
                shard_id,
                signer_account_id,
                signer_public_key,
                gas_price,
                action_kind,
                action_json,
                predecessor_id,
                receiver_id,
                block_hash,
                chunk_hash,
                author,
                method_name,
                gas,
                deposit,
                args_base64,
                args_json,
                action_index,
                block_timestamp
            )
            VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15,
                $16, $17, $18, $19, $20,
                $21, $22, $23
            )
            ON CONFLICT (id) DO UPDATE SET
                block_height = EXCLUDED.block_height,
                receipt_id = EXCLUDED.receipt_id,
                execution_outcome_id = EXCLUDED.execution_outcome_id,
                transaction_hash = COALESCE(EXCLUDED.transaction_hash, near.near_receipt_actions.transaction_hash),
                shard_id = EXCLUDED.shard_id,
                signer_account_id = EXCLUDED.signer_account_id,
                signer_public_key = EXCLUDED.signer_public_key,
                gas_price = EXCLUDED.gas_price,
                action_kind = EXCLUDED.action_kind,
                action_json = EXCLUDED.action_json,
                predecessor_id = EXCLUDED.predecessor_id,
                receiver_id = EXCLUDED.receiver_id,
                block_hash = EXCLUDED.block_hash,
                chunk_hash = EXCLUDED.chunk_hash,
                author = EXCLUDED.author,
                method_name = EXCLUDED.method_name,
                gas = EXCLUDED.gas,
                deposit = EXCLUDED.deposit,
                args_base64 = EXCLUDED.args_base64,
                args_json = EXCLUDED.args_json,
                action_index = EXCLUDED.action_index,
                block_timestamp = EXCLUDED.block_timestamp",
            vec![
                id.into(),
                block_height.into(),
                receipt_id.into(),
                execution_outcome_id.into(),
                transaction_hash.map(ToString::to_string).into(),
                shard_id.into(),
                signer_account_id.map(ToString::to_string).into(),
                signer_public_key.map(ToString::to_string).into(),
                gas_price.map(ToString::to_string).into(),
                action_kind.into(),
                action_json.into(),
                predecessor_id.into(),
                receiver_id.into(),
                block_hash.into(),
                chunk_hash.into(),
                author.into(),
                method_name.map(ToString::to_string).into(),
                gas.into(),
                deposit.map(ToString::to_string).into(),
                args_base64.map(ToString::to_string).into(),
                args_json.into(),
                action_index.into(),
                block_timestamp.into(),
            ],
        ))
        .await
        .context("Failed to upsert near.near_receipt_actions")?;

        Ok(())
    }

    async fn store_block(&self, block: &BlockView, db: &impl ConnectionTrait) -> Result<()> {
        let header = &block.header;

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
            raw_data: Set(Some(Self::serialize_json(block, "block raw_data")?)),
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
                let chunk_record = near_chunks::ActiveModel {
                    chunk_hash: Set(header.chunk_hash.to_string()),
                    block_height: Set(block_height),
                    shard_id: Set(Self::shard_to_i64(header.shard_id)),
                    author: Set(chunk.author.to_string()),
                    height_created: Set(header.height_created as i64),
                    height_included: Set(header.height_included as i64),
                    gas_used: Set(header.gas_used.as_gas() as i64),
                    gas_limit: Set(header.gas_limit.as_gas() as i64),
                    validator_reward: Set(header.validator_reward.to_string()),
                    raw_data: Set(Some(Self::serialize_json(chunk, "chunk raw_data")?)),
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

                    let transaction_record = near_transactions::ActiveModel {
                        hash: Set(tx.hash.to_string()),
                        block_height: Set(block_height),
                        shard_id: Set(Self::shard_to_i64(shard.shard_id)),
                        signer_id: Set(tx.signer_id.to_string()),
                        receiver_id: Set(tx.receiver_id.to_string()),
                        nonce: Set(tx.nonce as i64),
                        priority_fee: Set(tx.priority_fee as i64),
                        public_key: Set(tx.public_key.to_string()),
                        signature: Set(tx.signature.to_string()),
                        actions: Set(Self::serialize_json(&tx.actions, "transaction actions")?),
                        raw_data: Set(Some(Self::serialize_json(tx, "transaction raw_data")?)),
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
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        let receipt_record = near_receipts::ActiveModel {
                            receipt_id: Set(receipt.receipt_id.to_string()),
                            block_height: Set(block_height),
                            shard_id: Set(Self::shard_to_i64(shard.shard_id)),
                            predecessor_id: Set(receipt.predecessor_id.to_string()),
                            receiver_id: Set(receipt.receiver_id.to_string()),
                            receipt: Set(Self::serialize_json(
                                &receipt.receipt,
                                "transaction outcome receipt body",
                            )?),
                            raw_data: Set(Some(Self::serialize_json(
                                receipt,
                                "transaction outcome receipt raw_data",
                            )?)),
                        };

                        near_receipts::Entity::insert(receipt_record)
                            .on_conflict_do_nothing()
                            .exec(db)
                            .await
                            .context("Failed to insert tx receipt record")?;
                    }
                }

                for receipt in &chunk.receipts {
                    let receipt_record = near_receipts::ActiveModel {
                        receipt_id: Set(receipt.receipt_id.to_string()),
                        block_height: Set(block_height),
                        shard_id: Set(Self::shard_to_i64(shard.shard_id)),
                        predecessor_id: Set(receipt.predecessor_id.to_string()),
                        receiver_id: Set(receipt.receiver_id.to_string()),
                        receipt: Set(Self::serialize_json(
                            &receipt.receipt,
                            "chunk receipt body",
                        )?),
                        raw_data: Set(Some(Self::serialize_json(
                            receipt,
                            "chunk receipt raw_data",
                        )?)),
                    };

                    near_receipts::Entity::insert(receipt_record)
                        .on_conflict_do_nothing()
                        .exec(db)
                        .await
                        .context("Failed to insert chunk receipt record")?;
                }

                for receipt in &chunk.local_receipts {
                    let receipt_record = near_receipts::ActiveModel {
                        receipt_id: Set(receipt.receipt_id.to_string()),
                        block_height: Set(block_height),
                        shard_id: Set(Self::shard_to_i64(shard.shard_id)),
                        predecessor_id: Set(receipt.predecessor_id.to_string()),
                        receiver_id: Set(receipt.receiver_id.to_string()),
                        receipt: Set(Self::serialize_json(
                            &receipt.receipt,
                            "local receipt body",
                        )?),
                        raw_data: Set(Some(Self::serialize_json(
                            receipt,
                            "local receipt raw_data",
                        )?)),
                    };

                    near_receipts::Entity::insert(receipt_record)
                        .on_conflict_do_nothing()
                        .exec(db)
                        .await
                        .context("Failed to insert local receipt record")?;
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;

                let receipt_record = near_receipts::ActiveModel {
                    receipt_id: Set(receipt.receipt_id.to_string()),
                    block_height: Set(block_height),
                    shard_id: Set(Self::shard_to_i64(shard.shard_id)),
                    predecessor_id: Set(receipt.predecessor_id.to_string()),
                    receiver_id: Set(receipt.receiver_id.to_string()),
                    receipt: Set(Self::serialize_json(
                        &receipt.receipt,
                        "receipt execution outcome receipt body",
                    )?),
                    raw_data: Set(Some(Self::serialize_json(
                        receipt,
                        "receipt execution outcome receipt raw_data",
                    )?)),
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

    #[allow(clippy::too_many_arguments)]
    async fn store_execution_outcomes(
        &self,
        shards: &[IndexerShard],
        block_height: i64,
        block_hash: &str,
        block_timestamp: i64,
        default_author: &str,
        receipt_tx_hashes: &HashMap<String, String>,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let mut receipt_tx_hashes = receipt_tx_hashes.clone();
        let candidate_receipt_ids = Self::collect_candidate_receipt_ids(shards);
        let mapped_receipts = load_receipt_tx_hash_map(db, &candidate_receipt_ids).await?;
        for (receipt_id, transaction_hash) in mapped_receipts {
            receipt_tx_hashes
                .entry(receipt_id)
                .or_insert_with(|| transaction_hash);
        }

        let unresolved_receipt_ids = candidate_receipt_ids
            .iter()
            .filter(|receipt_id| !receipt_tx_hashes.contains_key(*receipt_id))
            .cloned()
            .collect::<Vec<_>>();
        if !unresolved_receipt_ids.is_empty() {
            let resolved_from_canonical =
                Self::resolve_receipt_tx_hashes_from_canonical_graph(db, &unresolved_receipt_ids)
                    .await?;
            for (receipt_id, transaction_hash) in resolved_from_canonical {
                Self::persist_receipt_tx_mapping(db, &receipt_id, &transaction_hash, block_height)
                    .await?;
                receipt_tx_hashes
                    .entry(receipt_id)
                    .or_insert_with(|| transaction_hash);
            }
        }

        for shard in shards {
            let shard_id = Self::shard_to_i64(shard.shard_id);
            let chunk_hash = shard
                .chunk
                .as_ref()
                .map(|chunk| chunk.header.chunk_hash.to_string())
                .unwrap_or_default();
            let chunk_author = shard
                .chunk
                .as_ref()
                .map(|chunk| chunk.author.to_string())
                .unwrap_or_else(|| default_author.to_string());

            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    let outcome_view = &tx_with_outcome.outcome.execution_outcome;
                    let outcome = &outcome_view.outcome;
                    let receipt_id = tx_with_outcome
                        .outcome
                        .receipt
                        .as_ref()
                        .map(|receipt| receipt.receipt_id.to_string());
                    let transaction_hash = tx_with_outcome.transaction.hash.to_string();
                    let outcome_receipt_ids = JsonValue::Array(
                        outcome
                            .receipt_ids
                            .iter()
                            .map(|value| JsonValue::String(value.to_string()))
                            .collect(),
                    );
                    let logs = JsonValue::Array(
                        outcome
                            .logs
                            .iter()
                            .cloned()
                            .map(JsonValue::String)
                            .collect(),
                    );
                    let (status, results_json) = Self::extract_status(&outcome.status);
                    let executor_account_id = outcome.executor_id.to_string();
                    let execution_outcome_id = outcome_view.id.to_string();

                    self.upsert_near_execution_outcome(
                        db,
                        receipt_id.as_deref(),
                        &execution_outcome_id,
                        Some(&transaction_hash),
                        block_height,
                        block_hash,
                        &chunk_hash,
                        shard_id,
                        outcome.gas_burnt.as_gas() as i64,
                        outcome.gas_burnt.as_gas() as f64,
                        outcome.tokens_burnt.as_yoctonear() as f64 / 1e24,
                        &executor_account_id,
                        status,
                        outcome_receipt_ids,
                        logs,
                        results_json,
                        Self::serialize_json(
                            &outcome_view.outcome,
                            "transaction execution outcome body",
                        )?,
                        Self::serialize_json(outcome_view, "transaction execution outcome row")?,
                        block_timestamp,
                    )
                    .await?;

                    if let Some(parent_receipt_id) = receipt_id.as_deref() {
                        receipt_tx_hashes
                            .entry(parent_receipt_id.to_string())
                            .or_insert_with(|| transaction_hash.clone());
                        Self::persist_receipt_tx_mapping(
                            db,
                            parent_receipt_id,
                            &transaction_hash,
                            block_height,
                        )
                        .await?;

                        for child_receipt_id in &outcome.receipt_ids {
                            let child_receipt_id_str = child_receipt_id.to_string();
                            self.upsert_near_receipt_edge(
                                db,
                                parent_receipt_id,
                                &child_receipt_id_str,
                                &execution_outcome_id,
                                block_height,
                                block_hash,
                                block_timestamp,
                                Some(&transaction_hash),
                            )
                            .await?;

                            receipt_tx_hashes
                                .entry(child_receipt_id_str.clone())
                                .or_insert_with(|| transaction_hash.clone());
                            Self::persist_receipt_tx_mapping(
                                db,
                                &child_receipt_id_str,
                                &transaction_hash,
                                block_height,
                            )
                            .await?;
                        }
                    } else {
                        for child_receipt_id in &outcome.receipt_ids {
                            let child_receipt_id_str = child_receipt_id.to_string();
                            receipt_tx_hashes
                                .entry(child_receipt_id_str.clone())
                                .or_insert_with(|| transaction_hash.clone());
                            Self::persist_receipt_tx_mapping(
                                db,
                                &child_receipt_id_str,
                                &transaction_hash,
                                block_height,
                            )
                            .await?;
                        }
                    }
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                let receipt_id = receipt.receipt_id.to_string();
                let transaction_hash = receipt_tx_hashes.get(&receipt_id).cloned();
                let outcome_view = &outcome_with_receipt.execution_outcome;
                let outcome = &outcome_view.outcome;
                let outcome_receipt_ids = JsonValue::Array(
                    outcome
                        .receipt_ids
                        .iter()
                        .map(|value| JsonValue::String(value.to_string()))
                        .collect(),
                );
                let logs = JsonValue::Array(
                    outcome
                        .logs
                        .iter()
                        .cloned()
                        .map(JsonValue::String)
                        .collect(),
                );
                let (status, results_json) = Self::extract_status(&outcome.status);
                let executor_account_id = outcome.executor_id.to_string();
                let execution_outcome_id = outcome_view.id.to_string();

                self.upsert_near_execution_outcome(
                    db,
                    Some(&receipt_id),
                    &execution_outcome_id,
                    transaction_hash.as_deref(),
                    block_height,
                    block_hash,
                    &chunk_hash,
                    shard_id,
                    outcome.gas_burnt.as_gas() as i64,
                    outcome.gas_burnt.as_gas() as f64,
                    outcome.tokens_burnt.as_yoctonear() as f64 / 1e24,
                    &executor_account_id,
                    status,
                    outcome_receipt_ids,
                    logs,
                    results_json,
                    Self::serialize_json(&outcome_view.outcome, "receipt execution outcome body")?,
                    Self::serialize_json(outcome_view, "receipt execution outcome row")?,
                    block_timestamp,
                )
                .await?;

                if let Some(transaction_hash) = &transaction_hash {
                    Self::persist_receipt_tx_mapping(
                        db,
                        &receipt_id,
                        transaction_hash,
                        block_height,
                    )
                    .await?;
                }

                for child_receipt_id in &outcome.receipt_ids {
                    let child_receipt_id_str = child_receipt_id.to_string();
                    self.upsert_near_receipt_edge(
                        db,
                        &receipt_id,
                        &child_receipt_id_str,
                        &execution_outcome_id,
                        block_height,
                        block_hash,
                        block_timestamp,
                        transaction_hash.as_deref(),
                    )
                    .await?;

                    if let Some(transaction_hash) = &transaction_hash {
                        receipt_tx_hashes
                            .entry(child_receipt_id_str.clone())
                            .or_insert_with(|| transaction_hash.clone());
                        Self::persist_receipt_tx_mapping(
                            db,
                            &child_receipt_id_str,
                            transaction_hash,
                            block_height,
                        )
                        .await?;
                    }
                }

                if let ReceiptEnumView::Action {
                    signer_id,
                    signer_public_key,
                    gas_price,
                    actions,
                    ..
                } = &receipt.receipt
                {
                    let predecessor_id_value = receipt.predecessor_id.to_string();
                    let receiver_id_value = receipt.receiver_id.to_string();

                    for (action_index, action) in actions.iter().enumerate() {
                        let action_json = Self::serialize_json(action, "receipt action")?;
                        let action_kind = Self::action_kind_from_json(&action_json);
                        let signer_id_value = signer_id.to_string();
                        let signer_public_key_value = signer_public_key.to_string();
                        let gas_price_value = gas_price.to_string();

                        let mut method_name: Option<String> = None;
                        let mut gas: Option<i64> = None;
                        let mut deposit: Option<String> = None;
                        let mut args_base64: Option<String> = None;
                        let mut args_json: Option<JsonValue> = None;

                        if let ActionView::FunctionCall {
                            method_name: function_name,
                            args,
                            gas: function_gas,
                            deposit: function_deposit,
                        } = action
                        {
                            let args_bytes: &[u8] = args.as_ref();
                            method_name = Some(function_name.clone());
                            gas = Some(function_gas.as_gas() as i64);
                            deposit = Some(function_deposit.to_string());
                            args_base64 = Some(BASE64_STANDARD.encode(args_bytes));
                            args_json = Some(Self::parse_args_json(args_bytes));
                        }

                        self.upsert_near_receipt_action(
                            db,
                            &format!("{receipt_id}-{action_index}"),
                            block_height,
                            &receipt_id,
                            &execution_outcome_id,
                            transaction_hash.as_deref(),
                            shard_id,
                            Some(&signer_id_value),
                            Some(&signer_public_key_value),
                            Some(&gas_price_value),
                            &predecessor_id_value,
                            &receiver_id_value,
                            block_hash,
                            &chunk_hash,
                            &chunk_author,
                            &action_kind,
                            action_json,
                            method_name.as_deref(),
                            gas,
                            deposit.as_deref(),
                            args_base64.as_deref(),
                            args_json,
                            action_index as i32,
                            block_timestamp,
                        )
                        .await?;
                    }
                }
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
            if shard.state_changes.is_empty() {
                continue;
            }

            let state_changes_record = near_state_changes::ActiveModel {
                id: sea_orm::ActiveValue::NotSet,
                block_height: Set(block_height),
                shard_id: Set(Self::shard_to_i64(shard.shard_id)),
                state_changes: Set(Self::serialize_json(
                    &shard.state_changes,
                    "state changes payload",
                )?),
            };

            near_state_changes::Entity::insert(state_changes_record)
                .on_conflict(
                    OnConflict::columns([
                        near_state_changes::Column::BlockHeight,
                        near_state_changes::Column::ShardId,
                    ])
                    .do_nothing()
                    .to_owned(),
                )
                .do_nothing()
                .exec(db)
                .await
                .context("Failed to insert state changes record")?;
        }

        Ok(())
    }

    async fn process_block_internal(
        &self,
        msg: &StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()> {
        let start_time = Instant::now();

        info!(
            module = MODULE_NAME,
            height = msg.block.header.height,
            shard_count = msg.shards.len(),
            "Processing NEAR raw block data"
        );

        let block_height = msg.block.header.height as i64;
        let block_hash = msg.block.header.hash.to_string();
        let block_timestamp = msg.block.header.timestamp as i64;
        let block_author = msg.block.author.to_string();
        let receipt_tx_hashes = build_receipt_tx_hashes(&msg.shards);

        let txn = db.begin().await?;

        self.store_block(&msg.block, &txn)
            .await
            .context("Failed to store block data")?;

        self.store_chunks(&msg.shards, block_height, &txn)
            .await
            .context("Failed to store chunk data")?;

        self.store_transactions(&msg.shards, block_height, &txn)
            .await
            .context("Failed to store transaction data")?;

        self.store_receipts(&msg.shards, block_height, &txn)
            .await
            .context("Failed to store receipt data")?;

        self.store_execution_outcomes(
            &msg.shards,
            block_height,
            &block_hash,
            block_timestamp,
            &block_author,
            &receipt_tx_hashes,
            &txn,
        )
        .await
        .context("Failed to store execution outcome data")?;

        self.store_state_changes(&msg.shards, block_height, &txn)
            .await
            .context("Failed to store state change data")?;

        txn.commit().await?;

        let elapsed = start_time.elapsed();
        info!(
            module = MODULE_NAME,
            height = msg.block.header.height,
            shard_count = msg.shards.len(),
            elapsed_ms = elapsed.as_millis(),
            "Successfully processed NEAR raw block data"
        );

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

    fn stream_mode(&self) -> NearModuleStreamMode {
        self.stream_mode
    }

    #[instrument(skip(self, msg, db), fields(module = "raw", height = %msg.block.header.height))]
    async fn process_block(&self, msg: &StreamerMessage, db: &DatabaseConnection) -> Result<()> {
        self.process_block_internal(msg, db).await
    }

    async fn get_cursor(&self, db: &DatabaseConnection) -> Result<Option<i64>> {
        cursor::get_module_cursor(db, MODULE_NAME).await
    }

    async fn update_cursor(
        &self,
        db: &DatabaseConnection,
        height: i64,
        prev_height: Option<i64>,
    ) -> Result<()> {
        cursor::upsert_module_cursor(db, MODULE_NAME, height, prev_height).await
    }
}
