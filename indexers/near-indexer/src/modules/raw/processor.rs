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
    ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, Set, Statement,
    TransactionTrait, Value,
};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;
use tracing::{error, info, instrument};

use crate::core::cursor;
use crate::core::receipt_tx_hash_map::load_receipt_tx_hash_map;
use crate::core::receipt_tx_resolver::build_receipt_tx_hashes;
use crate::core::stage_timing::BlockProcessingLog;
use crate::IndexerModule;

const MODULE_NAME: &str = "raw";
const MAX_BULK_INSERT_PARAMS: usize = 65_535;
const MAX_BULK_INSERT_ROWS: usize = 250;
const DATA_RECEIPT_LINK_RETENTION_BLOCKS: i64 = 256;

#[derive(Clone)]
struct ReceiptTxHashMapRow {
    receipt_id: String,
    transaction_hash: String,
    block_height: i64,
}

#[derive(Clone)]
struct NearChunkRow {
    chunk_hash: String,
    block_height: i64,
    shard_id: i64,
    author: String,
    height_created: i64,
    height_included: i64,
    gas_used: i64,
    gas_limit: i64,
    validator_reward: String,
    raw_data: JsonValue,
}

#[derive(Clone)]
struct NearStateChangesRow {
    block_height: i64,
    shard_id: i64,
    state_changes: JsonValue,
}

#[derive(Clone)]
struct NearTransactionRow {
    hash: String,
    block_height: i64,
    shard_id: i64,
    signer_id: String,
    receiver_id: String,
    nonce: i64,
    priority_fee: i64,
    public_key: String,
    signature: String,
    actions: JsonValue,
    raw_data: JsonValue,
}

#[derive(Clone)]
struct NearReceiptRow {
    receipt_id: String,
    block_height: i64,
    shard_id: i64,
    predecessor_id: String,
    receiver_id: String,
    receipt: JsonValue,
    raw_data: JsonValue,
}

#[derive(Clone)]
struct NearExecutionOutcomeRow {
    id: String,
    transaction_hash: Option<String>,
    block_height: i64,
    shard_id: i64,
    receipt_id: Option<String>,
    outcome: JsonValue,
    raw_data: JsonValue,
    block_hash: String,
    chunk_hash: String,
    gas_burnt: i64,
    gas_used: f64,
    tokens_burnt: f64,
    executor_account_id: String,
    status: String,
    outcome_receipt_ids: JsonValue,
    executed_in_block_hash: String,
    logs: JsonValue,
    results_json: Option<JsonValue>,
    block_timestamp: i64,
}

#[derive(Clone)]
struct NearReceiptActionRow {
    id: String,
    block_height: i64,
    receipt_id: String,
    execution_outcome_id: String,
    transaction_hash: Option<String>,
    shard_id: i64,
    signer_account_id: Option<String>,
    signer_public_key: Option<String>,
    gas_price: Option<String>,
    action_kind: String,
    action_json: JsonValue,
    predecessor_id: String,
    receiver_id: String,
    block_hash: String,
    chunk_hash: String,
    author: String,
    method_name: Option<String>,
    gas: Option<i64>,
    deposit: Option<String>,
    args_base64: Option<String>,
    args_json: Option<JsonValue>,
    action_index: i32,
    block_timestamp: i64,
}

#[derive(Clone)]
struct NearReceiptEdgeRow {
    parent_receipt_id: String,
    child_receipt_id: String,
    parent_execution_outcome_id: String,
    block_height: i64,
    block_hash: String,
    block_timestamp: i64,
    transaction_hash: Option<String>,
}

#[derive(Clone)]
struct DataReceiptParentLink {
    transaction_hash: Option<String>,
    parent_receipt_id: String,
    parent_execution_outcome_id: String,
    block_height: i64,
    block_hash: String,
    block_timestamp: i64,
}

#[derive(Default)]
struct RawRuntimeState {
    data_receipt_links: HashMap<String, DataReceiptParentLink>,
    data_receipt_link_heights: BTreeMap<i64, Vec<String>>,
}

impl RawRuntimeState {
    fn remember_data_receipt_links(
        &mut self,
        block_height: i64,
        links: HashMap<String, DataReceiptParentLink>,
    ) {
        if links.is_empty() {
            self.evict_data_receipt_links(block_height);
            return;
        }

        let height_bucket = self
            .data_receipt_link_heights
            .entry(block_height)
            .or_default();

        for (data_id, link) in links {
            height_bucket.push(data_id.clone());
            self.data_receipt_links.insert(data_id, link);
        }

        self.evict_data_receipt_links(block_height);
    }

    fn evict_data_receipt_links(&mut self, block_height: i64) {
        let cutoff_height = block_height.saturating_sub(DATA_RECEIPT_LINK_RETENTION_BLOCKS);
        let stale_heights = self
            .data_receipt_link_heights
            .keys()
            .copied()
            .take_while(|height| *height <= cutoff_height)
            .collect::<Vec<_>>();

        for height in stale_heights {
            if let Some(data_ids) = self.data_receipt_link_heights.remove(&height) {
                for data_id in data_ids {
                    let remove = self
                        .data_receipt_links
                        .get(&data_id)
                        .map(|link| link.block_height <= cutoff_height)
                        .unwrap_or(false);
                    if remove {
                        self.data_receipt_links.remove(&data_id);
                    }
                }
            }
        }
    }
}

pub struct RawModule {
    enabled: bool,
    stream_mode: NearModuleStreamMode,
    state: Mutex<RawRuntimeState>,
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
            state: Mutex::new(RawRuntimeState::default()),
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

    fn max_bulk_rows(columns_per_row: usize) -> usize {
        let max_by_params = (MAX_BULK_INSERT_PARAMS / columns_per_row.max(1)).max(1);
        max_by_params.min(MAX_BULK_INSERT_ROWS)
    }

    fn build_values_clause(row_count: usize, columns_per_row: usize) -> String {
        let mut sql = String::new();
        let mut placeholder = 1;
        for row_idx in 0..row_count {
            if row_idx > 0 {
                sql.push_str(", ");
            }
            sql.push('(');
            for col_idx in 0..columns_per_row {
                if col_idx > 0 {
                    sql.push_str(", ");
                }
                sql.push('$');
                sql.push_str(&placeholder.to_string());
                placeholder += 1;
            }
            sql.push(')');
        }
        sql
    }

    async fn execute_chunked_upsert<T, F>(
        db: &impl ConnectionTrait,
        rows: &[T],
        columns_per_row: usize,
        sql_prefix: &str,
        sql_suffix: &str,
        mut bind_row: F,
        context: &str,
    ) -> Result<()>
    where
        F: FnMut(&T, &mut Vec<Value>),
    {
        if rows.is_empty() {
            return Ok(());
        }

        let batch_size = Self::max_bulk_rows(columns_per_row);
        for chunk in rows.chunks(batch_size) {
            let mut sql = String::from(sql_prefix);
            sql.push_str(&Self::build_values_clause(chunk.len(), columns_per_row));
            sql.push_str(sql_suffix);

            let mut values = Vec::with_capacity(chunk.len() * columns_per_row);
            for row in chunk {
                bind_row(row, &mut values);
            }

            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &sql,
                values,
            ))
            .await
            .with_context(|| context.to_string())?;
        }

        Ok(())
    }

    async fn query_chunked<T, F>(
        db: &impl ConnectionTrait,
        rows: &[T],
        columns_per_row: usize,
        sql_prefix: &str,
        sql_suffix: &str,
        mut bind_row: F,
        context: &str,
    ) -> Result<Vec<sea_orm::QueryResult>>
    where
        F: FnMut(&T, &mut Vec<Value>),
    {
        if rows.is_empty() {
            return Ok(Vec::new());
        }

        let batch_size = Self::max_bulk_rows(columns_per_row);
        let mut results = Vec::new();
        for chunk in rows.chunks(batch_size) {
            let mut sql = String::from(sql_prefix);
            sql.push_str(&Self::build_values_clause(chunk.len(), columns_per_row));
            sql.push_str(sql_suffix);

            let mut values = Vec::with_capacity(chunk.len() * columns_per_row);
            for row in chunk {
                bind_row(row, &mut values);
            }

            let mut chunk_rows = db
                .query_all(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    &sql,
                    values,
                ))
                .await
                .with_context(|| context.to_string())?;
            results.append(&mut chunk_rows);
        }

        Ok(results)
    }

    fn note_receipt_tx_hash_mapping(
        mappings: &mut HashMap<String, ReceiptTxHashMapRow>,
        receipt_id: &str,
        transaction_hash: &str,
        block_height: i64,
    ) {
        mappings
            .entry(receipt_id.to_string())
            .and_modify(|row| {
                if row.transaction_hash != transaction_hash {
                    error!(
                        module = MODULE_NAME,
                        receipt_id = %receipt_id,
                        incoming_transaction_hash = %transaction_hash,
                        persisted_transaction_hash = %row.transaction_hash,
                        "receipt->tx hash conflict detected in block write set; preserving first-seen mapping"
                    );
                }
                row.block_height = row.block_height.max(block_height);
            })
            .or_insert_with(|| ReceiptTxHashMapRow {
                receipt_id: receipt_id.to_string(),
                transaction_hash: transaction_hash.to_string(),
                block_height,
            });
    }

    fn note_output_data_receivers(
        links: &mut HashMap<String, DataReceiptParentLink>,
        receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
        transaction_hash: Option<&str>,
        parent_execution_outcome_id: &str,
        block_height: i64,
        block_hash: &str,
        block_timestamp: i64,
    ) {
        let ReceiptEnumView::Action {
            output_data_receivers,
            ..
        } = &receipt.receipt
        else {
            return;
        };

        for data_receiver in output_data_receivers {
            links
                .entry(data_receiver.data_id.to_string())
                .or_insert_with(|| DataReceiptParentLink {
                    transaction_hash: transaction_hash.map(str::to_string),
                    parent_receipt_id: receipt.receipt_id.to_string(),
                    parent_execution_outcome_id: parent_execution_outcome_id.to_string(),
                    block_height,
                    block_hash: block_hash.to_string(),
                    block_timestamp,
                });
        }
    }

    fn data_receipt_link<'a>(
        links: &'a HashMap<String, DataReceiptParentLink>,
        receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
    ) -> Option<&'a DataReceiptParentLink> {
        let ReceiptEnumView::Data { data_id, .. } = &receipt.receipt else {
            return None;
        };
        links.get(&data_id.to_string())
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

    async fn flush_receipt_tx_hash_map_upserts(
        db: &impl ConnectionTrait,
        rows: &[ReceiptTxHashMapRow],
    ) -> Result<()> {
        let query_rows = Self::query_chunked(
            db,
            rows,
            3,
            "INSERT INTO near.near_receipt_tx_hash_map (
                receipt_id,
                transaction_hash,
                first_seen_height,
                last_seen_height,
                created_at,
                updated_at
            )
            SELECT
                incoming.receipt_id,
                incoming.transaction_hash,
                incoming.block_height,
                incoming.block_height,
                NOW(),
                NOW()
            FROM (VALUES ",
            "
            ) AS incoming(receipt_id, transaction_hash, block_height)
            ON CONFLICT (receipt_id) DO UPDATE SET
                last_seen_height = GREATEST(
                    near.near_receipt_tx_hash_map.last_seen_height,
                    EXCLUDED.last_seen_height
                ),
                updated_at = NOW()
            RETURNING receipt_id, transaction_hash",
            |row, values| {
                values.push(row.receipt_id.clone().into());
                values.push(row.transaction_hash.clone().into());
                values.push(row.block_height.into());
            },
            "failed to bulk upsert near_receipt_tx_hash_map",
        )
        .await?;

        let mut persisted = HashMap::new();
        for row in query_rows {
            let receipt_id: String = row
                .try_get("", "receipt_id")
                .context("failed to read bulk near_receipt_tx_hash_map.receipt_id")?;
            let transaction_hash: String = row
                .try_get("", "transaction_hash")
                .context("failed to read bulk near_receipt_tx_hash_map.transaction_hash")?;
            persisted.insert(receipt_id, transaction_hash);
        }

        for row in rows {
            if let Some(persisted_tx_hash) = persisted.get(&row.receipt_id) {
                if persisted_tx_hash != &row.transaction_hash {
                    error!(
                        module = MODULE_NAME,
                        receipt_id = %row.receipt_id,
                        incoming_transaction_hash = %row.transaction_hash,
                        persisted_transaction_hash = %persisted_tx_hash,
                        "receipt->tx hash conflict detected; preserving first-seen mapping"
                    );
                }
            }
        }

        Ok(())
    }

    async fn flush_near_chunks(db: &impl ConnectionTrait, rows: &[NearChunkRow]) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            10,
            "INSERT INTO near.near_chunks (
                chunk_hash,
                block_height,
                shard_id,
                author,
                height_created,
                height_included,
                gas_used,
                gas_limit,
                validator_reward,
                raw_data
            ) VALUES ",
            "
            ON CONFLICT (chunk_hash) DO NOTHING",
            |row, values| {
                values.push(row.chunk_hash.clone().into());
                values.push(row.block_height.into());
                values.push(row.shard_id.into());
                values.push(row.author.clone().into());
                values.push(row.height_created.into());
                values.push(row.height_included.into());
                values.push(row.gas_used.into());
                values.push(row.gas_limit.into());
                values.push(row.validator_reward.clone().into());
                values.push(row.raw_data.clone().into());
            },
            "failed to bulk insert near_chunks",
        )
        .await
    }

    async fn flush_near_transactions(
        db: &impl ConnectionTrait,
        rows: &[NearTransactionRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            11,
            "INSERT INTO near.near_transactions (
                hash,
                block_height,
                signer_id,
                receiver_id,
                nonce,
                priority_fee,
                public_key,
                signature,
                actions,
                raw_data,
                shard_id
            ) VALUES ",
            "
            ON CONFLICT (hash) DO NOTHING",
            |row, values| {
                values.push(row.hash.clone().into());
                values.push(row.block_height.into());
                values.push(row.signer_id.clone().into());
                values.push(row.receiver_id.clone().into());
                values.push(row.nonce.into());
                values.push(row.priority_fee.into());
                values.push(row.public_key.clone().into());
                values.push(row.signature.clone().into());
                values.push(row.actions.clone().into());
                values.push(row.raw_data.clone().into());
                values.push(row.shard_id.into());
            },
            "failed to bulk insert near_transactions",
        )
        .await
    }

    async fn flush_near_receipts(db: &impl ConnectionTrait, rows: &[NearReceiptRow]) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            7,
            "INSERT INTO near.near_receipts (
                receipt_id,
                block_height,
                shard_id,
                predecessor_id,
                receiver_id,
                receipt,
                raw_data
            ) VALUES ",
            "
            ON CONFLICT (receipt_id) DO NOTHING",
            |row, values| {
                values.push(row.receipt_id.clone().into());
                values.push(row.block_height.into());
                values.push(row.shard_id.into());
                values.push(row.predecessor_id.clone().into());
                values.push(row.receiver_id.clone().into());
                values.push(row.receipt.clone().into());
                values.push(row.raw_data.clone().into());
            },
            "failed to bulk insert near_receipts",
        )
        .await
    }

    async fn flush_near_execution_outcomes(
        db: &impl ConnectionTrait,
        rows: &[NearExecutionOutcomeRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            19,
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
            ) VALUES ",
            "
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
            |row, values| {
                values.push(row.id.clone().into());
                values.push(row.block_height.into());
                values.push(row.shard_id.into());
                values.push(row.receipt_id.clone().into());
                values.push(row.outcome.clone().into());
                values.push(row.raw_data.clone().into());
                values.push(row.transaction_hash.clone().into());
                values.push(row.block_hash.clone().into());
                values.push(row.chunk_hash.clone().into());
                values.push(row.gas_burnt.into());
                values.push(row.gas_used.into());
                values.push(row.tokens_burnt.into());
                values.push(row.executor_account_id.clone().into());
                values.push(row.status.clone().into());
                values.push(row.outcome_receipt_ids.clone().into());
                values.push(row.executed_in_block_hash.clone().into());
                values.push(row.logs.clone().into());
                values.push(row.results_json.clone().into());
                values.push(row.block_timestamp.into());
            },
            "failed to bulk upsert near_execution_outcomes",
        )
        .await
    }

    async fn flush_near_receipt_actions(
        db: &impl ConnectionTrait,
        rows: &[NearReceiptActionRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            23,
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
            ) VALUES ",
            "
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
            |row, values| {
                values.push(row.id.clone().into());
                values.push(row.block_height.into());
                values.push(row.receipt_id.clone().into());
                values.push(row.execution_outcome_id.clone().into());
                values.push(row.transaction_hash.clone().into());
                values.push(row.shard_id.into());
                values.push(row.signer_account_id.clone().into());
                values.push(row.signer_public_key.clone().into());
                values.push(row.gas_price.clone().into());
                values.push(row.action_kind.clone().into());
                values.push(row.action_json.clone().into());
                values.push(row.predecessor_id.clone().into());
                values.push(row.receiver_id.clone().into());
                values.push(row.block_hash.clone().into());
                values.push(row.chunk_hash.clone().into());
                values.push(row.author.clone().into());
                values.push(row.method_name.clone().into());
                values.push(row.gas.into());
                values.push(row.deposit.clone().into());
                values.push(row.args_base64.clone().into());
                values.push(row.args_json.clone().into());
                values.push(row.action_index.into());
                values.push(row.block_timestamp.into());
            },
            "failed to bulk upsert near_receipt_actions",
        )
        .await
    }

    async fn flush_near_receipt_edges(
        db: &impl ConnectionTrait,
        rows: &[NearReceiptEdgeRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            7,
            "INSERT INTO near.near_receipt_edges (
                parent_receipt_id,
                child_receipt_id,
                parent_execution_outcome_id,
                block_height,
                block_hash,
                block_timestamp,
                transaction_hash
            ) VALUES ",
            "
            ON CONFLICT (parent_receipt_id, child_receipt_id) DO UPDATE SET
                parent_execution_outcome_id = EXCLUDED.parent_execution_outcome_id,
                block_height = EXCLUDED.block_height,
                block_hash = EXCLUDED.block_hash,
                block_timestamp = EXCLUDED.block_timestamp,
                transaction_hash = COALESCE(
                    near.near_receipt_edges.transaction_hash,
                    EXCLUDED.transaction_hash
                )",
            |row, values| {
                values.push(row.parent_receipt_id.clone().into());
                values.push(row.child_receipt_id.clone().into());
                values.push(row.parent_execution_outcome_id.clone().into());
                values.push(row.block_height.into());
                values.push(row.block_hash.clone().into());
                values.push(row.block_timestamp.into());
                values.push(row.transaction_hash.clone().into());
            },
            "failed to bulk upsert near_receipt_edges",
        )
        .await
    }

    async fn flush_near_state_changes(
        db: &impl ConnectionTrait,
        rows: &[NearStateChangesRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            3,
            "INSERT INTO near.near_state_changes (
                block_height,
                shard_id,
                state_changes
            ) VALUES ",
            "
            ON CONFLICT (block_height, shard_id) DO NOTHING",
            |row, values| {
                values.push(row.block_height.into());
                values.push(row.shard_id.into());
                values.push(row.state_changes.clone().into());
            },
            "failed to bulk insert near_state_changes",
        )
        .await
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

    #[allow(dead_code, clippy::too_many_arguments)]
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

    #[allow(dead_code, clippy::too_many_arguments)]
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

    #[allow(dead_code, clippy::too_many_arguments)]
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
        let mut rows = Vec::new();
        for shard in shards {
            if let Some(chunk) = &shard.chunk {
                let header = &chunk.header;
                rows.push(NearChunkRow {
                    chunk_hash: header.chunk_hash.to_string(),
                    block_height,
                    shard_id: Self::shard_to_i64(header.shard_id),
                    author: chunk.author.to_string(),
                    height_created: header.height_created as i64,
                    height_included: header.height_included as i64,
                    gas_used: header.gas_used.as_gas() as i64,
                    gas_limit: header.gas_limit.as_gas() as i64,
                    validator_reward: header.validator_reward.to_string(),
                    raw_data: Self::serialize_json(chunk, "chunk raw_data")?,
                });
            }
        }

        Self::flush_near_chunks(db, &rows).await?;
        Ok(())
    }

    async fn store_transactions(
        &self,
        shards: &[IndexerShard],
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let mut rows = Vec::new();
        for shard in shards {
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    let tx = &tx_with_outcome.transaction;
                    rows.push(NearTransactionRow {
                        hash: tx.hash.to_string(),
                        block_height,
                        shard_id: Self::shard_to_i64(shard.shard_id),
                        signer_id: tx.signer_id.to_string(),
                        receiver_id: tx.receiver_id.to_string(),
                        nonce: tx.nonce as i64,
                        priority_fee: tx.priority_fee as i64,
                        public_key: tx.public_key.to_string(),
                        signature: tx.signature.to_string(),
                        actions: Self::serialize_json(&tx.actions, "transaction actions")?,
                        raw_data: Self::serialize_json(tx, "transaction raw_data")?,
                    });
                }
            }
        }

        Self::flush_near_transactions(db, &rows).await?;
        Ok(())
    }

    async fn store_receipts(
        &self,
        shards: &[IndexerShard],
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let mut rows = HashMap::new();
        for shard in shards {
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        rows.entry(receipt.receipt_id.to_string())
                            .or_insert(NearReceiptRow {
                                receipt_id: receipt.receipt_id.to_string(),
                                block_height,
                                shard_id: Self::shard_to_i64(shard.shard_id),
                                predecessor_id: receipt.predecessor_id.to_string(),
                                receiver_id: receipt.receiver_id.to_string(),
                                receipt: Self::serialize_json(
                                    &receipt.receipt,
                                    "transaction outcome receipt body",
                                )?,
                                raw_data: Self::serialize_json(
                                    receipt,
                                    "transaction outcome receipt raw_data",
                                )?,
                            });
                    }
                }

                for receipt in &chunk.receipts {
                    rows.entry(receipt.receipt_id.to_string())
                        .or_insert(NearReceiptRow {
                            receipt_id: receipt.receipt_id.to_string(),
                            block_height,
                            shard_id: Self::shard_to_i64(shard.shard_id),
                            predecessor_id: receipt.predecessor_id.to_string(),
                            receiver_id: receipt.receiver_id.to_string(),
                            receipt: Self::serialize_json(&receipt.receipt, "chunk receipt body")?,
                            raw_data: Self::serialize_json(receipt, "chunk receipt raw_data")?,
                        });
                }

                for receipt in &chunk.local_receipts {
                    rows.entry(receipt.receipt_id.to_string())
                        .or_insert(NearReceiptRow {
                            receipt_id: receipt.receipt_id.to_string(),
                            block_height,
                            shard_id: Self::shard_to_i64(shard.shard_id),
                            predecessor_id: receipt.predecessor_id.to_string(),
                            receiver_id: receipt.receiver_id.to_string(),
                            receipt: Self::serialize_json(&receipt.receipt, "local receipt body")?,
                            raw_data: Self::serialize_json(receipt, "local receipt raw_data")?,
                        });
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                rows.entry(receipt.receipt_id.to_string())
                    .or_insert(NearReceiptRow {
                        receipt_id: receipt.receipt_id.to_string(),
                        block_height,
                        shard_id: Self::shard_to_i64(shard.shard_id),
                        predecessor_id: receipt.predecessor_id.to_string(),
                        receiver_id: receipt.receiver_id.to_string(),
                        receipt: Self::serialize_json(
                            &receipt.receipt,
                            "receipt execution outcome receipt body",
                        )?,
                        raw_data: Self::serialize_json(
                            receipt,
                            "receipt execution outcome receipt raw_data",
                        )?,
                    });
            }
        }

        let rows = rows.into_values().collect::<Vec<_>>();
        Self::flush_near_receipts(db, &rows).await?;
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
        let mut data_receipt_links = {
            let state = self.state.lock().unwrap();
            state.data_receipt_links.clone()
        };
        let mut new_data_receipt_links = HashMap::new();
        let mut outcome_rows = HashMap::new();
        let mut edge_rows = HashMap::new();
        let mut action_rows = HashMap::new();
        let mut receipt_tx_rows = HashMap::new();
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
                Self::note_receipt_tx_hash_mapping(
                    &mut receipt_tx_rows,
                    &receipt_id,
                    &transaction_hash,
                    block_height,
                );
                receipt_tx_hashes
                    .entry(receipt_id)
                    .or_insert_with(|| transaction_hash);
            }
        }

        for shard in shards {
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    let Some(receipt) = &tx_with_outcome.outcome.receipt else {
                        continue;
                    };
                    let transaction_hash = tx_with_outcome.transaction.hash.to_string();
                    let execution_outcome_id =
                        tx_with_outcome.outcome.execution_outcome.id.to_string();
                    Self::note_output_data_receivers(
                        &mut data_receipt_links,
                        receipt,
                        Some(&transaction_hash),
                        &execution_outcome_id,
                        block_height,
                        block_hash,
                        block_timestamp,
                    );
                    Self::note_output_data_receivers(
                        &mut new_data_receipt_links,
                        receipt,
                        Some(&transaction_hash),
                        &execution_outcome_id,
                        block_height,
                        block_hash,
                        block_timestamp,
                    );
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                let receipt_id = receipt.receipt_id.to_string();
                let data_receipt_link =
                    Self::data_receipt_link(&data_receipt_links, receipt).cloned();
                let mut transaction_hash = receipt_tx_hashes.get(&receipt_id).cloned();
                if transaction_hash.is_none() {
                    transaction_hash = data_receipt_link
                        .as_ref()
                        .and_then(|link| link.transaction_hash.clone());
                }
                let execution_outcome_id = outcome_with_receipt.execution_outcome.id.to_string();

                Self::note_output_data_receivers(
                    &mut data_receipt_links,
                    receipt,
                    transaction_hash.as_deref(),
                    &execution_outcome_id,
                    block_height,
                    block_hash,
                    block_timestamp,
                );
                Self::note_output_data_receivers(
                    &mut new_data_receipt_links,
                    receipt,
                    transaction_hash.as_deref(),
                    &execution_outcome_id,
                    block_height,
                    block_hash,
                    block_timestamp,
                );
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

                    outcome_rows.insert(
                        execution_outcome_id.clone(),
                        NearExecutionOutcomeRow {
                            id: execution_outcome_id.clone(),
                            transaction_hash: Some(transaction_hash.clone()),
                            block_height,
                            shard_id,
                            receipt_id: receipt_id.clone(),
                            outcome: Self::serialize_json(
                                &outcome_view.outcome,
                                "transaction execution outcome body",
                            )?,
                            raw_data: Self::serialize_json(
                                outcome_view,
                                "transaction execution outcome row",
                            )?,
                            block_hash: block_hash.to_string(),
                            chunk_hash: chunk_hash.clone(),
                            gas_burnt: outcome.gas_burnt.as_gas() as i64,
                            gas_used: outcome.gas_burnt.as_gas() as f64,
                            tokens_burnt: outcome.tokens_burnt.as_yoctonear() as f64 / 1e24,
                            executor_account_id,
                            status: status.to_string(),
                            outcome_receipt_ids,
                            executed_in_block_hash: block_hash.to_string(),
                            logs,
                            results_json,
                            block_timestamp,
                        },
                    );

                    if let Some(parent_receipt_id) = receipt_id.as_deref() {
                        receipt_tx_hashes
                            .entry(parent_receipt_id.to_string())
                            .or_insert_with(|| transaction_hash.clone());
                        Self::note_receipt_tx_hash_mapping(
                            &mut receipt_tx_rows,
                            parent_receipt_id,
                            &transaction_hash,
                            block_height,
                        );

                        for child_receipt_id in &outcome.receipt_ids {
                            let child_receipt_id_str = child_receipt_id.to_string();
                            edge_rows.insert(
                                (parent_receipt_id.to_string(), child_receipt_id_str.clone()),
                                NearReceiptEdgeRow {
                                    parent_receipt_id: parent_receipt_id.to_string(),
                                    child_receipt_id: child_receipt_id_str.clone(),
                                    parent_execution_outcome_id: execution_outcome_id.clone(),
                                    block_height,
                                    block_hash: block_hash.to_string(),
                                    block_timestamp,
                                    transaction_hash: Some(transaction_hash.clone()),
                                },
                            );

                            receipt_tx_hashes
                                .entry(child_receipt_id_str.clone())
                                .or_insert_with(|| transaction_hash.clone());
                            Self::note_receipt_tx_hash_mapping(
                                &mut receipt_tx_rows,
                                &child_receipt_id_str,
                                &transaction_hash,
                                block_height,
                            );
                        }

                        if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                            Self::note_output_data_receivers(
                                &mut data_receipt_links,
                                receipt,
                                Some(&transaction_hash),
                                &execution_outcome_id,
                                block_height,
                                block_hash,
                                block_timestamp,
                            );
                            Self::note_output_data_receivers(
                                &mut new_data_receipt_links,
                                receipt,
                                Some(&transaction_hash),
                                &execution_outcome_id,
                                block_height,
                                block_hash,
                                block_timestamp,
                            );
                        }
                    } else {
                        for child_receipt_id in &outcome.receipt_ids {
                            let child_receipt_id_str = child_receipt_id.to_string();
                            receipt_tx_hashes
                                .entry(child_receipt_id_str.clone())
                                .or_insert_with(|| transaction_hash.clone());
                            Self::note_receipt_tx_hash_mapping(
                                &mut receipt_tx_rows,
                                &child_receipt_id_str,
                                &transaction_hash,
                                block_height,
                            );
                        }
                    }
                }

                for receipt in chunk.receipts.iter().chain(chunk.local_receipts.iter()) {
                    let receipt_id = receipt.receipt_id.to_string();
                    let data_receipt_link =
                        Self::data_receipt_link(&data_receipt_links, receipt).cloned();
                    let mut transaction_hash = receipt_tx_hashes.get(&receipt_id).cloned();
                    if transaction_hash.is_none() {
                        transaction_hash = data_receipt_link
                            .as_ref()
                            .and_then(|link| link.transaction_hash.clone());
                    }

                    if let Some(transaction_hash) = &transaction_hash {
                        receipt_tx_hashes
                            .entry(receipt_id.clone())
                            .or_insert_with(|| transaction_hash.clone());
                        Self::note_receipt_tx_hash_mapping(
                            &mut receipt_tx_rows,
                            &receipt_id,
                            transaction_hash,
                            block_height,
                        );
                    }

                    if let Some(link) = data_receipt_link.as_ref() {
                        edge_rows.insert(
                            (link.parent_receipt_id.clone(), receipt_id.clone()),
                            NearReceiptEdgeRow {
                                parent_receipt_id: link.parent_receipt_id.clone(),
                                child_receipt_id: receipt_id.clone(),
                                parent_execution_outcome_id: link
                                    .parent_execution_outcome_id
                                    .clone(),
                                block_height: link.block_height,
                                block_hash: link.block_hash.clone(),
                                block_timestamp: link.block_timestamp,
                                transaction_hash: transaction_hash.clone(),
                            },
                        );
                    }
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                let receipt_id = receipt.receipt_id.to_string();
                let data_receipt_link =
                    Self::data_receipt_link(&data_receipt_links, receipt).cloned();
                let mut transaction_hash = receipt_tx_hashes.get(&receipt_id).cloned();
                if transaction_hash.is_none() {
                    transaction_hash = data_receipt_link
                        .as_ref()
                        .and_then(|link| link.transaction_hash.clone());
                }
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

                outcome_rows.insert(
                    execution_outcome_id.clone(),
                    NearExecutionOutcomeRow {
                        id: execution_outcome_id.clone(),
                        transaction_hash: transaction_hash.clone(),
                        block_height,
                        shard_id,
                        receipt_id: Some(receipt_id.clone()),
                        outcome: Self::serialize_json(
                            &outcome_view.outcome,
                            "receipt execution outcome body",
                        )?,
                        raw_data: Self::serialize_json(
                            outcome_view,
                            "receipt execution outcome row",
                        )?,
                        block_hash: block_hash.to_string(),
                        chunk_hash: chunk_hash.clone(),
                        gas_burnt: outcome.gas_burnt.as_gas() as i64,
                        gas_used: outcome.gas_burnt.as_gas() as f64,
                        tokens_burnt: outcome.tokens_burnt.as_yoctonear() as f64 / 1e24,
                        executor_account_id,
                        status: status.to_string(),
                        outcome_receipt_ids: outcome_receipt_ids.clone(),
                        executed_in_block_hash: block_hash.to_string(),
                        logs: logs.clone(),
                        results_json: results_json.clone(),
                        block_timestamp,
                    },
                );

                if let Some(transaction_hash) = &transaction_hash {
                    receipt_tx_hashes
                        .entry(receipt_id.clone())
                        .or_insert_with(|| transaction_hash.clone());
                    Self::note_receipt_tx_hash_mapping(
                        &mut receipt_tx_rows,
                        &receipt_id,
                        transaction_hash,
                        block_height,
                    );
                }

                if let Some(link) = data_receipt_link.as_ref() {
                    edge_rows.insert(
                        (link.parent_receipt_id.clone(), receipt_id.clone()),
                        NearReceiptEdgeRow {
                            parent_receipt_id: link.parent_receipt_id.clone(),
                            child_receipt_id: receipt_id.clone(),
                            parent_execution_outcome_id: link.parent_execution_outcome_id.clone(),
                            block_height: link.block_height,
                            block_hash: link.block_hash.clone(),
                            block_timestamp: link.block_timestamp,
                            transaction_hash: transaction_hash.clone(),
                        },
                    );
                }

                for child_receipt_id in &outcome.receipt_ids {
                    let child_receipt_id_str = child_receipt_id.to_string();
                    edge_rows.insert(
                        (receipt_id.clone(), child_receipt_id_str.clone()),
                        NearReceiptEdgeRow {
                            parent_receipt_id: receipt_id.clone(),
                            child_receipt_id: child_receipt_id_str.clone(),
                            parent_execution_outcome_id: execution_outcome_id.clone(),
                            block_height,
                            block_hash: block_hash.to_string(),
                            block_timestamp,
                            transaction_hash: transaction_hash.clone(),
                        },
                    );

                    if let Some(transaction_hash) = &transaction_hash {
                        receipt_tx_hashes
                            .entry(child_receipt_id_str.clone())
                            .or_insert_with(|| transaction_hash.clone());
                        Self::note_receipt_tx_hash_mapping(
                            &mut receipt_tx_rows,
                            &child_receipt_id_str,
                            transaction_hash,
                            block_height,
                        );
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

                        action_rows.insert(
                            format!("{receipt_id}-{action_index}"),
                            NearReceiptActionRow {
                                id: format!("{receipt_id}-{action_index}"),
                                block_height,
                                receipt_id: receipt_id.clone(),
                                execution_outcome_id: execution_outcome_id.clone(),
                                transaction_hash: transaction_hash.clone(),
                                shard_id,
                                signer_account_id: Some(signer_id_value),
                                signer_public_key: Some(signer_public_key_value),
                                gas_price: Some(gas_price_value),
                                action_kind,
                                action_json,
                                predecessor_id: predecessor_id_value.clone(),
                                receiver_id: receiver_id_value.clone(),
                                block_hash: block_hash.to_string(),
                                chunk_hash: chunk_hash.clone(),
                                author: chunk_author.clone(),
                                method_name,
                                gas,
                                deposit,
                                args_base64,
                                args_json,
                                action_index: action_index as i32,
                                block_timestamp,
                            },
                        );
                    }
                }
            }
        }

        let outcome_rows = outcome_rows.into_values().collect::<Vec<_>>();
        let edge_rows = edge_rows.into_values().collect::<Vec<_>>();
        let action_rows = action_rows.into_values().collect::<Vec<_>>();
        let receipt_tx_rows = receipt_tx_rows.into_values().collect::<Vec<_>>();

        Self::flush_near_execution_outcomes(db, &outcome_rows).await?;
        Self::flush_near_receipt_edges(db, &edge_rows).await?;
        Self::flush_near_receipt_actions(db, &action_rows).await?;
        Self::flush_receipt_tx_hash_map_upserts(db, &receipt_tx_rows).await?;

        {
            let mut state = self.state.lock().unwrap();
            state.remember_data_receipt_links(block_height, new_data_receipt_links);
        }

        Ok(())
    }

    async fn store_state_changes(
        &self,
        shards: &[IndexerShard],
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let mut rows = Vec::new();
        for shard in shards {
            if shard.state_changes.is_empty() {
                continue;
            }

            rows.push(NearStateChangesRow {
                block_height,
                shard_id: Self::shard_to_i64(shard.shard_id),
                state_changes: Self::serialize_json(&shard.state_changes, "state changes payload")?,
            });
        }

        Self::flush_near_state_changes(db, &rows).await?;
        Ok(())
    }

    async fn process_block_internal(
        &self,
        msg: &StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()> {
        let height = msg.block.header.height;
        let shard_count = msg.shards.len();
        info!(
            module = MODULE_NAME,
            height = height,
            shard_count = shard_count,
            "Processing NEAR raw block data"
        );

        let mut process_log = BlockProcessingLog::new(MODULE_NAME, height, shard_count);
        process_log.metrics_mut().record_count(
            "chunk_count",
            msg.shards
                .iter()
                .filter(|shard| shard.chunk.is_some())
                .count(),
        );
        process_log.metrics_mut().record_count(
            "transaction_count",
            msg.shards
                .iter()
                .filter_map(|shard| shard.chunk.as_ref())
                .map(|chunk| chunk.transactions.len())
                .sum::<usize>(),
        );
        process_log.metrics_mut().record_count(
            "receipt_count",
            msg.shards
                .iter()
                .filter_map(|shard| shard.chunk.as_ref())
                .map(|chunk| chunk.receipts.len() + chunk.local_receipts.len())
                .sum::<usize>(),
        );
        process_log.metrics_mut().record_count(
            "receipt_execution_outcome_count",
            msg.shards
                .iter()
                .map(|shard| shard.receipt_execution_outcomes.len())
                .sum::<usize>(),
        );
        process_log.metrics_mut().record_count(
            "state_change_count",
            msg.shards
                .iter()
                .map(|shard| shard.state_changes.len())
                .sum::<usize>(),
        );

        let block_height = height as i64;
        let block_hash = msg.block.header.hash.to_string();
        let block_timestamp = msg.block.header.timestamp as i64;
        let block_author = msg.block.author.to_string();
        let receipt_tx_hashes_start = std::time::Instant::now();
        let receipt_tx_hashes = build_receipt_tx_hashes(&msg.shards);
        process_log
            .metrics_mut()
            .record_phase_elapsed("build_receipt_tx_hashes", receipt_tx_hashes_start);

        let txn = process_log
            .metrics_mut()
            .time_future("txn_begin", db.begin())
            .await?;

        process_log
            .metrics_mut()
            .time_future("store_block", self.store_block(&msg.block, &txn))
            .await
            .context("Failed to store block data")?;

        process_log
            .metrics_mut()
            .time_future(
                "store_chunks",
                self.store_chunks(&msg.shards, block_height, &txn),
            )
            .await
            .context("Failed to store chunk data")?;

        process_log
            .metrics_mut()
            .time_future(
                "store_transactions",
                self.store_transactions(&msg.shards, block_height, &txn),
            )
            .await
            .context("Failed to store transaction data")?;

        process_log
            .metrics_mut()
            .time_future(
                "store_receipts",
                self.store_receipts(&msg.shards, block_height, &txn),
            )
            .await
            .context("Failed to store receipt data")?;

        process_log
            .metrics_mut()
            .time_future(
                "store_execution_outcomes",
                self.store_execution_outcomes(
                    &msg.shards,
                    block_height,
                    &block_hash,
                    block_timestamp,
                    &block_author,
                    &receipt_tx_hashes,
                    &txn,
                ),
            )
            .await
            .context("Failed to store execution outcome data")?;

        process_log
            .metrics_mut()
            .time_future(
                "store_state_changes",
                self.store_state_changes(&msg.shards, block_height, &txn),
            )
            .await
            .context("Failed to store state change data")?;

        process_log
            .metrics_mut()
            .time_future("txn_commit", txn.commit())
            .await?;

        process_log.log_success("Successfully processed NEAR raw block data");

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

#[cfg(test)]
mod tests {
    use super::{RawModule, RawRuntimeState};
    use near_lake_framework::near_indexer_primitives::types::Balance;
    use near_lake_framework::near_indexer_primitives::views::{
        DataReceiverView, ReceiptEnumView, ReceiptView,
    };
    use near_lake_framework::near_indexer_primitives::CryptoHash;

    fn crypto_hash(byte: u8) -> CryptoHash {
        CryptoHash([byte; 32])
    }

    fn action_receipt_with_data_outputs(data_ids: &[u8]) -> ReceiptView {
        ReceiptView {
            predecessor_id: "sputnik-dao.near".parse().unwrap(),
            receiver_id: "hos-root.sputnik-dao.near".parse().unwrap(),
            receipt_id: crypto_hash(1),
            receipt: ReceiptEnumView::Action {
                signer_id: "lane.near".parse().unwrap(),
                signer_public_key: "ed25519:2duQfvJJJRuhknFFcSqPsQcxCeMt8UsoLqQ9mzmyrYHz"
                    .parse()
                    .unwrap(),
                gas_price: Balance::from_yoctonear(0),
                output_data_receivers: data_ids
                    .iter()
                    .map(|byte| DataReceiverView {
                        data_id: crypto_hash(*byte),
                        receiver_id: "sputnik-dao.near".parse().unwrap(),
                    })
                    .collect(),
                input_data_ids: Vec::new(),
                actions: Vec::new(),
                is_promise_yield: false,
                refund_to: None,
            },
            priority: 0,
        }
    }

    fn data_receipt(data_id: u8) -> ReceiptView {
        ReceiptView {
            predecessor_id: "hos-root.sputnik-dao.near".parse().unwrap(),
            receiver_id: "sputnik-dao.near".parse().unwrap(),
            receipt_id: crypto_hash(2),
            receipt: ReceiptEnumView::Data {
                data_id: crypto_hash(data_id),
                data: Some(Vec::new()),
                is_promise_resume: false,
            },
            priority: 0,
        }
    }

    #[test]
    fn raw_runtime_retains_data_receipt_links_across_blocks() {
        let parent_receipt = action_receipt_with_data_outputs(&[7]);
        let mut links = std::collections::HashMap::new();
        RawModule::note_output_data_receivers(
            &mut links,
            &parent_receipt,
            Some("tx-1"),
            "outcome-1",
            100,
            "block-100",
            1_000,
        );

        let mut state = RawRuntimeState::default();
        state.remember_data_receipt_links(100, links);

        let link = RawModule::data_receipt_link(&state.data_receipt_links, &data_receipt(7))
            .expect("expected cached data receipt link");
        assert_eq!(link.transaction_hash.as_deref(), Some("tx-1"));
        assert_eq!(
            link.parent_receipt_id,
            parent_receipt.receipt_id.to_string()
        );
        assert_eq!(link.parent_execution_outcome_id, "outcome-1");
        assert_eq!(link.block_height, 100);
    }
}
