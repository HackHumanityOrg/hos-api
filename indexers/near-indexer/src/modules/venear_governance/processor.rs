use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use common::config::NearModuleStreamMode;
use near_lake_framework::near_indexer_primitives::views::{
    ActionView, ExecutionStatusView, ReceiptEnumView,
};
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, TransactionTrait,
    Value,
};
use serde_json::{json, Value as JsonValue};
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tracing::{error, info, instrument, warn};

use crate::core::cursor;
use crate::core::receipt_tx_hash_map::load_receipt_tx_hash_map;
use crate::core::receipt_tx_resolver::build_receipt_tx_hashes;
use crate::core::stage_timing::{BlockProcessingLog, StageMetrics};
use crate::IndexerModule;

const MODULE_NAME: &str = "venear_governance";
const VENEAR_CONTRACT_ID: &str = "venear.dao";
const VOTING_CONTRACT_ID: &str = "vote.dao";
const HOS_ROOT_CONTRACT_ID: &str = "hos-root.sputnik-dao.near";
const HOS_REWARDS_CONTRACT_ID: &str = "rewards.hos-dao.near";
const VENEAR_LOCKUP_SUFFIX: &str = ".venear.dao";
const DEFAULT_HOS_MVIEW_REFRESH_INTERVAL_BLOCKS: i64 = 100;
const DEFAULT_STAGING_GC_INTERVAL_BLOCKS: i64 = 600;
const DEFAULT_STAGING_GC_BATCH_SIZE: i64 = 5_000;
const DEFAULT_STAGING_GC_MAX_BATCHES: i64 = 50;
const DEFAULT_TX_MAP_CLEANUP_INTERVAL_BLOCKS: i64 = 600;
const DEFAULT_TX_MAP_RETENTION_BLOCKS: i64 = 86_400;
const DEFAULT_TX_MAP_CLEANUP_BATCH_SIZE: i64 = 5_000;
const DEFAULT_TX_MAP_CLEANUP_MAX_BATCHES: i64 = 50;
const STALLED_PENDING_GRAPH_WARNING_BLOCKS: i64 = 43_200;
const TX_STATE_GC_RETENTION_BLOCKS: i64 = 43_200;
const TX_STATUS_PENDING: &str = "pending";
const TX_STATUS_FINALIZED: &str = "finalized";
const TX_STATUS_PRUNED_NON_HOS: &str = "pruned_non_hos";
const TX_STATUS_INCOMPLETE_MISSING_TX_HASH: &str = "incomplete_missing_tx_hash";
const MAX_BULK_INSERT_PARAMS: usize = 65_535;
// Bulk statements repeat large JSON payloads per row, especially for staged
// block/chunk/state-change raw_data. Keep row batches small enough to avoid
// oversized Postgres statements while still reducing round trips materially.
const MAX_BULK_INSERT_ROWS: usize = 25;

#[derive(Clone)]
struct TxStateUpsertRow {
    transaction_hash: String,
    has_hos_touch: bool,
    block_height: i64,
}

#[derive(Clone)]
struct ReceiptTxHashMapRow {
    receipt_id: String,
    transaction_hash: String,
    block_height: i64,
}

#[derive(Clone)]
struct StagingBlockRow {
    transaction_hash: String,
    height: i64,
    hash: String,
    prev_hash: String,
    epoch_id: String,
    next_epoch_id: String,
    author: String,
    timestamp: i64,
    timestamp_nanosec: i64,
    gas_price: String,
    total_supply: String,
    validator_reward: String,
    protocol_version: i32,
    chunks_included: i64,
    block_ordinal: Option<i64>,
    prev_height: Option<i64>,
    raw_data: JsonValue,
    seen_height: i64,
}

#[derive(Clone)]
struct StagingChunkRow {
    transaction_hash: String,
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
struct StagingStateChangesRow {
    transaction_hash: String,
    block_height: i64,
    shard_id: i64,
    state_changes: JsonValue,
}

#[derive(Clone)]
struct StagingTransactionRow {
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
struct StagingReceiptRow {
    receipt_id: String,
    transaction_hash: String,
    block_height: i64,
    shard_id: i64,
    predecessor_id: String,
    receiver_id: String,
    receipt: JsonValue,
    raw_data: JsonValue,
}

#[derive(Clone)]
struct StagingExecutionOutcomeRow {
    id: String,
    transaction_hash: String,
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
struct StagingReceiptActionRow {
    id: String,
    transaction_hash: String,
    block_height: i64,
    receipt_id: String,
    execution_outcome_id: String,
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
struct StagingReceiptEdgeRow {
    parent_receipt_id: String,
    child_receipt_id: String,
    transaction_hash: String,
    parent_execution_outcome_id: String,
    block_height: i64,
    block_hash: String,
    block_timestamp: i64,
}

#[derive(Clone)]
struct StagingReceiptStatusRow {
    receipt_id: String,
    transaction_hash: String,
    executed: bool,
    block_height: i64,
}

#[derive(Default)]
struct HosBlockWrites {
    tx_states: HashMap<String, TxStateUpsertRow>,
    receipt_tx_hash_map: HashMap<String, ReceiptTxHashMapRow>,
    staging_blocks: HashMap<String, StagingBlockRow>,
    staging_chunks: HashMap<(String, String), StagingChunkRow>,
    staging_state_changes: HashMap<(String, i64, i64), StagingStateChangesRow>,
    staging_transactions: HashMap<String, StagingTransactionRow>,
    staging_receipts: HashMap<String, StagingReceiptRow>,
    staging_execution_outcomes: HashMap<String, StagingExecutionOutcomeRow>,
    staging_receipt_actions: HashMap<String, StagingReceiptActionRow>,
    staging_receipt_edges: HashMap<(String, String), StagingReceiptEdgeRow>,
    staging_receipt_statuses: HashMap<String, StagingReceiptStatusRow>,
}

impl HosBlockWrites {
    fn note_tx_state(&mut self, transaction_hash: &str, block_height: i64, has_hos_touch: bool) {
        self.tx_states
            .entry(transaction_hash.to_string())
            .and_modify(|row| {
                row.has_hos_touch |= has_hos_touch;
                row.block_height = row.block_height.max(block_height);
            })
            .or_insert_with(|| TxStateUpsertRow {
                transaction_hash: transaction_hash.to_string(),
                has_hos_touch,
                block_height,
            });
    }

    fn note_receipt_tx_hash_mapping(
        &mut self,
        receipt_id: &str,
        transaction_hash: &str,
        block_height: i64,
    ) {
        self.receipt_tx_hash_map
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

    fn note_staging_block(&mut self, row: StagingBlockRow) {
        self.staging_blocks
            .entry(row.transaction_hash.clone())
            .or_insert(row);
    }

    fn note_staging_chunk(&mut self, row: StagingChunkRow) {
        self.staging_chunks
            .entry((row.transaction_hash.clone(), row.chunk_hash.clone()))
            .or_insert(row);
    }

    fn note_staging_state_changes(&mut self, row: StagingStateChangesRow) {
        self.staging_state_changes
            .entry((row.transaction_hash.clone(), row.block_height, row.shard_id))
            .or_insert(row);
    }

    fn note_staging_transaction(&mut self, row: StagingTransactionRow) {
        self.staging_transactions
            .entry(row.hash.clone())
            .or_insert(row);
    }

    fn note_staging_receipt(&mut self, row: StagingReceiptRow) {
        self.staging_receipts
            .entry(row.receipt_id.clone())
            .or_insert(row);
    }

    fn note_staging_execution_outcome(&mut self, row: StagingExecutionOutcomeRow) {
        self.staging_execution_outcomes
            .entry(row.id.clone())
            .and_modify(|existing| {
                if existing.receipt_id.is_none() {
                    existing.receipt_id = row.receipt_id.clone();
                }
            })
            .or_insert(row);
    }

    fn note_staging_receipt_action(&mut self, row: StagingReceiptActionRow) {
        self.staging_receipt_actions
            .entry(row.id.clone())
            .or_insert(row);
    }

    fn note_staging_receipt_edge(&mut self, row: StagingReceiptEdgeRow) {
        self.staging_receipt_edges
            .entry((row.parent_receipt_id.clone(), row.child_receipt_id.clone()))
            .or_insert(row);
    }

    fn note_staging_receipt_status(&mut self, row: StagingReceiptStatusRow) {
        self.staging_receipt_statuses
            .entry(row.receipt_id.clone())
            .and_modify(|existing| {
                existing.transaction_hash = row.transaction_hash.clone();
                existing.executed |= row.executed;
                existing.block_height = existing.block_height.max(row.block_height);
            })
            .or_insert(row);
    }
}

pub struct VenearGovernanceModule {
    enabled: bool,
    stream_mode: NearModuleStreamMode,
    mview_refresh_interval_blocks: i64,
    staging_gc_interval_blocks: i64,
    staging_gc_batch_size: i64,
    staging_gc_max_batches_per_run: i64,
    receipt_tx_map_cleanup_interval_blocks: i64,
    receipt_tx_map_retention_blocks: i64,
    receipt_tx_map_cleanup_batch_size: i64,
    receipt_tx_map_cleanup_max_batches_per_run: i64,
    raw_enabled: bool,
}

impl VenearGovernanceModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        let mut enabled = false;
        let mut stream_mode = NearModuleStreamMode::Shared;
        let mut mview_refresh_interval_blocks = DEFAULT_HOS_MVIEW_REFRESH_INTERVAL_BLOCKS;
        let mut staging_gc_interval_blocks = DEFAULT_STAGING_GC_INTERVAL_BLOCKS;
        let mut staging_gc_batch_size = DEFAULT_STAGING_GC_BATCH_SIZE;
        let mut staging_gc_max_batches_per_run = DEFAULT_STAGING_GC_MAX_BATCHES;
        let mut receipt_tx_map_cleanup_interval_blocks = DEFAULT_TX_MAP_CLEANUP_INTERVAL_BLOCKS;
        let mut receipt_tx_map_retention_blocks = DEFAULT_TX_MAP_RETENTION_BLOCKS;
        let mut receipt_tx_map_cleanup_batch_size = DEFAULT_TX_MAP_CLEANUP_BATCH_SIZE;
        let mut receipt_tx_map_cleanup_max_batches_per_run = DEFAULT_TX_MAP_CLEANUP_MAX_BATCHES;
        let mut raw_enabled = false;

        if let common::config::AppConfig::Indexer(idx) = config {
            let idx = idx.as_ref();
            if let Some(near) = idx.near.as_ref() {
                let near_enabled = near.chain != "disabled";
                if near_enabled {
                    raw_enabled = near.raw_enabled_or_default();
                    if let Some(module_cfg) = near
                        .modules
                        .as_ref()
                        .and_then(|m| m.venear_governance.as_ref())
                    {
                        enabled = module_cfg.enabled;
                        stream_mode = module_cfg.stream_mode;
                        mview_refresh_interval_blocks =
                            i64::try_from(module_cfg.mview_refresh_interval_blocks)
                                .unwrap_or(i64::MAX);
                        staging_gc_interval_blocks =
                            i64::try_from(module_cfg.staging_gc_interval_blocks)
                                .unwrap_or(i64::MAX);
                        staging_gc_batch_size =
                            i64::try_from(module_cfg.staging_gc_batch_size).unwrap_or(i64::MAX);
                        staging_gc_max_batches_per_run =
                            i64::try_from(module_cfg.staging_gc_max_batches_per_run)
                                .unwrap_or(i64::MAX);
                        receipt_tx_map_cleanup_interval_blocks =
                            i64::try_from(module_cfg.receipt_tx_map_cleanup_interval_blocks)
                                .unwrap_or(i64::MAX);
                        receipt_tx_map_retention_blocks =
                            i64::try_from(module_cfg.receipt_tx_map_retention_blocks)
                                .unwrap_or(i64::MAX);
                        receipt_tx_map_cleanup_batch_size =
                            i64::try_from(module_cfg.receipt_tx_map_cleanup_batch_size)
                                .unwrap_or(i64::MAX);
                        receipt_tx_map_cleanup_max_batches_per_run =
                            i64::try_from(module_cfg.receipt_tx_map_cleanup_max_batches_per_run)
                                .unwrap_or(i64::MAX);
                    }
                }
            }
        }

        Self {
            enabled,
            stream_mode,
            mview_refresh_interval_blocks,
            staging_gc_interval_blocks,
            staging_gc_batch_size,
            staging_gc_max_batches_per_run,
            receipt_tx_map_cleanup_interval_blocks,
            receipt_tx_map_retention_blocks,
            receipt_tx_map_cleanup_batch_size,
            receipt_tx_map_cleanup_max_batches_per_run,
            raw_enabled,
        }
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
            .unwrap_or_else(|_| json!({ "value": "invalid json" }))
    }

    fn serialize_json<T: serde::Serialize>(value: &T, label: &str) -> Result<JsonValue> {
        serde_json::to_value(value).with_context(|| format!("failed to serialize {label}"))
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
                Some(json!({
                    "status_type": "Unknown",
                })),
            ),
            ExecutionStatusView::Failure(failure) => (
                "Failure",
                Some(json!({
                    "error": failure.to_string(),
                    "status_type": "Failure",
                })),
            ),
            ExecutionStatusView::SuccessReceiptId(receipt_id) => (
                "SuccessReceiptId",
                Some(json!({
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
                    .unwrap_or_else(|_| json!({ "value": "invalid json" }));

                ("SuccessValue", Some(parsed))
            }
        }
    }

    fn shard_to_i64(shard_id: near_lake_framework::near_indexer_primitives::types::ShardId) -> i64 {
        let value: u64 = shard_id.into();
        i64::try_from(value).unwrap_or(i64::MAX)
    }

    async fn load_receipt_tx_from_staging_status(
        db: &impl ConnectionTrait,
        receipt_ids: &[String],
    ) -> Result<HashMap<String, String>> {
        let mut deduped = Vec::new();
        let mut seen = HashSet::new();
        for receipt_id in receipt_ids {
            if seen.insert(receipt_id.clone()) {
                deduped.push(receipt_id.clone());
            }
        }

        if deduped.is_empty() {
            return Ok(HashMap::new());
        }

        let mut placeholders = String::new();
        for idx in 0..deduped.len() {
            if idx > 0 {
                placeholders.push_str(", ");
            }
            placeholders.push_str(&format!("${}", idx + 1));
        }

        let sql = format!(
            "SELECT receipt_id, transaction_hash
             FROM near.near_hos_staging_receipt_status
             WHERE receipt_id IN ({placeholders})"
        );

        let rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &sql,
                deduped.into_iter().map(Into::into).collect::<Vec<_>>(),
            ))
            .await
            .context("failed to load near_hos_staging_receipt_status")?;

        let mut result = HashMap::new();
        for row in rows {
            let receipt_id: String = row
                .try_get("", "receipt_id")
                .context("failed to read near_hos_staging_receipt_status.receipt_id")?;
            let transaction_hash: String = row
                .try_get("", "transaction_hash")
                .context("failed to read near_hos_staging_receipt_status.transaction_hash")?;
            result.insert(receipt_id, transaction_hash);
        }

        Ok(result)
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
    ) -> Result<Vec<QueryResult>>
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

    async fn flush_tx_state_upserts(
        db: &impl ConnectionTrait,
        rows: &[TxStateUpsertRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            3,
            "INSERT INTO near.near_hos_tx_state (
                graph_key,
                transaction_hash,
                status,
                pending_receipt_count,
                has_hos_touch,
                first_seen_height,
                last_seen_height,
                created_at,
                updated_at
            )
            SELECT
                incoming.transaction_hash,
                incoming.transaction_hash,
                'pending',
                0,
                incoming.has_hos_touch,
                incoming.block_height,
                incoming.block_height,
                NOW(),
                NOW()
            FROM (VALUES ",
            "
            ) AS incoming(transaction_hash, has_hos_touch, block_height)
            ON CONFLICT (graph_key) DO UPDATE SET
                last_seen_height = GREATEST(
                    near.near_hos_tx_state.last_seen_height,
                    EXCLUDED.last_seen_height
                ),
                has_hos_touch = near.near_hos_tx_state.has_hos_touch OR EXCLUDED.has_hos_touch,
                status = EXCLUDED.status,
                finalized_at = NULL,
                representative_receipt_id = NULL,
                incomplete = FALSE,
                error_reason = NULL,
                updated_at = NOW()",
            |row, values| {
                values.push(row.transaction_hash.clone().into());
                values.push(row.has_hos_touch.into());
                values.push(row.block_height.into());
            },
            "failed to bulk upsert near_hos_tx_state",
        )
        .await
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

    async fn mark_incomplete_missing_tx_hash(
        db: &impl ConnectionTrait,
        receipt_id: &str,
        block_height: i64,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_hos_tx_state (
                graph_key,
                transaction_hash,
                status,
                pending_receipt_count,
                has_hos_touch,
                first_seen_height,
                last_seen_height,
                representative_receipt_id,
                incomplete,
                error_reason,
                created_at,
                updated_at
            ) VALUES ($1, NULL, $2, 1, TRUE, $3, $3, $4, TRUE, $5, NOW(), NOW())
            ON CONFLICT (graph_key) DO UPDATE SET
                last_seen_height = GREATEST(
                    near.near_hos_tx_state.last_seen_height,
                    EXCLUDED.last_seen_height
                ),
                pending_receipt_count = EXCLUDED.pending_receipt_count,
                has_hos_touch = TRUE,
                representative_receipt_id = EXCLUDED.representative_receipt_id,
                finalized_at = NULL,
                incomplete = TRUE,
                error_reason = EXCLUDED.error_reason,
                status = EXCLUDED.status,
                updated_at = NOW()",
            vec![
                format!("missing_tx::{receipt_id}").into(),
                TX_STATUS_INCOMPLETE_MISSING_TX_HASH.into(),
                block_height.into(),
                receipt_id.into(),
                "unable_to_resolve_transaction_hash".into(),
            ],
        ))
        .await
        .context("failed to mark missing tx hash incomplete state")?;

        Ok(())
    }

    fn is_hos_lockup_contract(account_id: &str) -> bool {
        account_id
            .strip_suffix(VENEAR_LOCKUP_SUFFIX)
            .map(|prefix| {
                prefix.len() == 40
                    && prefix
                        .as_bytes()
                        .iter()
                        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
            })
            .unwrap_or(false)
    }

    fn is_hos_contract(account_id: &str) -> bool {
        let account = account_id.to_ascii_lowercase();
        matches!(
            account.as_str(),
            VENEAR_CONTRACT_ID
                | VOTING_CONTRACT_ID
                | HOS_ROOT_CONTRACT_ID
                | HOS_REWARDS_CONTRACT_ID
        ) || Self::is_hos_lockup_contract(&account)
    }

    fn is_hos_related_with_signer(
        &self,
        receiver_id: &str,
        predecessor_id: &str,
        signer_account_id: Option<&str>,
    ) -> bool {
        Self::is_hos_contract(receiver_id)
            || Self::is_hos_contract(predecessor_id)
            || signer_account_id
                .map(Self::is_hos_contract)
                .unwrap_or(false)
    }

    fn is_hos_related_receipt(
        &self,
        receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
    ) -> bool {
        self.is_hos_related_with_signer(
            receipt.receiver_id.as_ref(),
            receipt.predecessor_id.as_ref(),
            Self::signer_account_id(&receipt.receipt),
        )
    }

    fn signer_account_id(receipt: &ReceiptEnumView) -> Option<&str> {
        match receipt {
            ReceiptEnumView::Action { signer_id, .. } => Some(signer_id.as_ref()),
            _ => None,
        }
    }

    async fn refresh_hos_materialized_views(&self, db: &DatabaseConnection) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT near.refresh_hos_materialized_views()",
            vec![],
        ))
        .await
        .context("failed refreshing HOS materialized views")?;

        Ok(())
    }

    fn should_refresh_mviews(&self, block_height: i64) -> bool {
        self.mview_refresh_interval_blocks > 0
            && block_height % self.mview_refresh_interval_blocks == 0
    }

    async fn flush_staging_block_upserts(
        db: &impl ConnectionTrait,
        rows: &[StagingBlockRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            19,
            "INSERT INTO near.near_hos_staging_blocks (
                transaction_hash,
                height,
                hash,
                prev_hash,
                epoch_id,
                next_epoch_id,
                author,
                timestamp,
                timestamp_nanosec,
                gas_price,
                total_supply,
                validator_reward,
                protocol_version,
                chunks_included,
                block_ordinal,
                prev_height,
                raw_data,
                first_seen_height,
                last_seen_height
            ) VALUES ",
            "
            ON CONFLICT (transaction_hash, height) DO UPDATE SET
                last_seen_height = GREATEST(
                    near.near_hos_staging_blocks.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            |row, values| {
                values.push(row.transaction_hash.clone().into());
                values.push(row.height.into());
                values.push(row.hash.clone().into());
                values.push(row.prev_hash.clone().into());
                values.push(row.epoch_id.clone().into());
                values.push(row.next_epoch_id.clone().into());
                values.push(row.author.clone().into());
                values.push(row.timestamp.into());
                values.push(row.timestamp_nanosec.into());
                values.push(row.gas_price.clone().into());
                values.push(row.total_supply.clone().into());
                values.push(row.validator_reward.clone().into());
                values.push(row.protocol_version.into());
                values.push(row.chunks_included.into());
                values.push(row.block_ordinal.into());
                values.push(row.prev_height.into());
                values.push(row.raw_data.clone().into());
                values.push(row.seen_height.into());
                values.push(row.seen_height.into());
            },
            "failed to bulk upsert near_hos_staging_blocks",
        )
        .await
    }

    async fn flush_staging_chunk_upserts(
        db: &impl ConnectionTrait,
        rows: &[StagingChunkRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            13,
            "INSERT INTO near.near_hos_staging_chunks (
                transaction_hash,
                chunk_hash,
                block_height,
                shard_id,
                author,
                height_created,
                height_included,
                gas_used,
                gas_limit,
                validator_reward,
                raw_data,
                first_seen_height,
                last_seen_height
            ) VALUES ",
            "
            ON CONFLICT (transaction_hash, chunk_hash) DO UPDATE SET
                block_height = EXCLUDED.block_height,
                shard_id = EXCLUDED.shard_id,
                author = EXCLUDED.author,
                height_created = EXCLUDED.height_created,
                height_included = EXCLUDED.height_included,
                gas_used = EXCLUDED.gas_used,
                gas_limit = EXCLUDED.gas_limit,
                validator_reward = EXCLUDED.validator_reward,
                raw_data = COALESCE(
                    EXCLUDED.raw_data,
                    near.near_hos_staging_chunks.raw_data
                ),
                last_seen_height = GREATEST(
                    near.near_hos_staging_chunks.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            |row, values| {
                values.push(row.transaction_hash.clone().into());
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
                values.push(row.block_height.into());
                values.push(row.block_height.into());
            },
            "failed to bulk upsert near_hos_staging_chunks",
        )
        .await
    }

    async fn flush_staging_state_changes_upserts(
        db: &impl ConnectionTrait,
        rows: &[StagingStateChangesRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            6,
            "INSERT INTO near.near_hos_staging_state_changes (
                transaction_hash,
                block_height,
                shard_id,
                state_changes,
                first_seen_height,
                last_seen_height
            ) VALUES ",
            "
            ON CONFLICT (transaction_hash, block_height, shard_id) DO UPDATE SET
                state_changes = EXCLUDED.state_changes,
                last_seen_height = GREATEST(
                    near.near_hos_staging_state_changes.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            |row, values| {
                values.push(row.transaction_hash.clone().into());
                values.push(row.block_height.into());
                values.push(row.shard_id.into());
                values.push(row.state_changes.clone().into());
                values.push(row.block_height.into());
                values.push(row.block_height.into());
            },
            "failed to bulk upsert near_hos_staging_state_changes",
        )
        .await
    }

    async fn flush_staging_transaction_upserts(
        db: &impl ConnectionTrait,
        rows: &[StagingTransactionRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            13,
            "INSERT INTO near.near_hos_staging_transactions (
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
                shard_id,
                first_seen_height,
                last_seen_height
            ) VALUES ",
            "
            ON CONFLICT (hash) DO UPDATE SET
                block_height = EXCLUDED.block_height,
                signer_id = EXCLUDED.signer_id,
                receiver_id = EXCLUDED.receiver_id,
                nonce = EXCLUDED.nonce,
                priority_fee = EXCLUDED.priority_fee,
                public_key = EXCLUDED.public_key,
                signature = EXCLUDED.signature,
                actions = EXCLUDED.actions,
                raw_data = COALESCE(
                    EXCLUDED.raw_data,
                    near.near_hos_staging_transactions.raw_data
                ),
                shard_id = EXCLUDED.shard_id,
                last_seen_height = GREATEST(
                    near.near_hos_staging_transactions.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
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
                values.push(row.block_height.into());
                values.push(row.block_height.into());
            },
            "failed to bulk upsert near_hos_staging_transactions",
        )
        .await
    }

    async fn flush_staging_receipt_upserts(
        db: &impl ConnectionTrait,
        rows: &[StagingReceiptRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            10,
            "INSERT INTO near.near_hos_staging_receipts (
                receipt_id,
                transaction_hash,
                block_height,
                shard_id,
                predecessor_id,
                receiver_id,
                receipt,
                raw_data,
                first_seen_height,
                last_seen_height
            ) VALUES ",
            "
            ON CONFLICT (receipt_id) DO UPDATE SET
                transaction_hash = EXCLUDED.transaction_hash,
                block_height = EXCLUDED.block_height,
                shard_id = EXCLUDED.shard_id,
                predecessor_id = EXCLUDED.predecessor_id,
                receiver_id = EXCLUDED.receiver_id,
                receipt = EXCLUDED.receipt,
                raw_data = COALESCE(
                    near.near_hos_staging_receipts.raw_data,
                    EXCLUDED.raw_data
                ),
                last_seen_height = GREATEST(
                    near.near_hos_staging_receipts.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            |row, values| {
                values.push(row.receipt_id.clone().into());
                values.push(row.transaction_hash.clone().into());
                values.push(row.block_height.into());
                values.push(row.shard_id.into());
                values.push(row.predecessor_id.clone().into());
                values.push(row.receiver_id.clone().into());
                values.push(row.receipt.clone().into());
                values.push(row.raw_data.clone().into());
                values.push(row.block_height.into());
                values.push(row.block_height.into());
            },
            "failed to bulk upsert near_hos_staging_receipts",
        )
        .await
    }

    async fn flush_staging_execution_outcome_upserts(
        db: &impl ConnectionTrait,
        rows: &[StagingExecutionOutcomeRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            21,
            "INSERT INTO near.near_hos_staging_execution_outcomes (
                id,
                transaction_hash,
                block_height,
                shard_id,
                receipt_id,
                outcome,
                raw_data,
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
                block_timestamp,
                first_seen_height,
                last_seen_height
            ) VALUES ",
            "
            ON CONFLICT (id) DO UPDATE SET
                transaction_hash = EXCLUDED.transaction_hash,
                block_height = EXCLUDED.block_height,
                shard_id = EXCLUDED.shard_id,
                receipt_id = COALESCE(
                    near.near_hos_staging_execution_outcomes.receipt_id,
                    EXCLUDED.receipt_id
                ),
                outcome = EXCLUDED.outcome,
                raw_data = COALESCE(
                    near.near_hos_staging_execution_outcomes.raw_data,
                    EXCLUDED.raw_data
                ),
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
                block_timestamp = EXCLUDED.block_timestamp,
                last_seen_height = GREATEST(
                    near.near_hos_staging_execution_outcomes.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            |row, values| {
                values.push(row.id.clone().into());
                values.push(row.transaction_hash.clone().into());
                values.push(row.block_height.into());
                values.push(row.shard_id.into());
                values.push(row.receipt_id.clone().into());
                values.push(row.outcome.clone().into());
                values.push(row.raw_data.clone().into());
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
                values.push(row.block_height.into());
                values.push(row.block_height.into());
            },
            "failed to bulk upsert near_hos_staging_execution_outcomes",
        )
        .await
    }

    async fn flush_staging_receipt_action_upserts(
        db: &impl ConnectionTrait,
        rows: &[StagingReceiptActionRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            25,
            "INSERT INTO near.near_hos_staging_receipt_actions (
                id,
                transaction_hash,
                block_height,
                receipt_id,
                execution_outcome_id,
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
                block_timestamp,
                first_seen_height,
                last_seen_height
            ) VALUES ",
            "
            ON CONFLICT (id) DO UPDATE SET
                transaction_hash = EXCLUDED.transaction_hash,
                block_height = EXCLUDED.block_height,
                receipt_id = EXCLUDED.receipt_id,
                execution_outcome_id = EXCLUDED.execution_outcome_id,
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
                block_timestamp = EXCLUDED.block_timestamp,
                last_seen_height = GREATEST(
                    near.near_hos_staging_receipt_actions.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            |row, values| {
                values.push(row.id.clone().into());
                values.push(row.transaction_hash.clone().into());
                values.push(row.block_height.into());
                values.push(row.receipt_id.clone().into());
                values.push(row.execution_outcome_id.clone().into());
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
                values.push(row.block_height.into());
                values.push(row.block_height.into());
            },
            "failed to bulk upsert near_hos_staging_receipt_actions",
        )
        .await
    }

    async fn flush_staging_receipt_edge_upserts(
        db: &impl ConnectionTrait,
        rows: &[StagingReceiptEdgeRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            9,
            "INSERT INTO near.near_hos_staging_receipt_edges (
                parent_receipt_id,
                child_receipt_id,
                transaction_hash,
                parent_execution_outcome_id,
                block_height,
                block_hash,
                block_timestamp,
                first_seen_height,
                last_seen_height
            ) VALUES ",
            "
            ON CONFLICT (parent_receipt_id, child_receipt_id) DO UPDATE SET
                transaction_hash = EXCLUDED.transaction_hash,
                parent_execution_outcome_id = EXCLUDED.parent_execution_outcome_id,
                block_height = EXCLUDED.block_height,
                block_hash = EXCLUDED.block_hash,
                block_timestamp = EXCLUDED.block_timestamp,
                last_seen_height = GREATEST(
                    near.near_hos_staging_receipt_edges.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            |row, values| {
                values.push(row.parent_receipt_id.clone().into());
                values.push(row.child_receipt_id.clone().into());
                values.push(row.transaction_hash.clone().into());
                values.push(row.parent_execution_outcome_id.clone().into());
                values.push(row.block_height.into());
                values.push(row.block_hash.clone().into());
                values.push(row.block_timestamp.into());
                values.push(row.block_height.into());
                values.push(row.block_height.into());
            },
            "failed to bulk upsert near_hos_staging_receipt_edges",
        )
        .await
    }

    async fn flush_staging_receipt_status_upserts(
        db: &impl ConnectionTrait,
        rows: &[StagingReceiptStatusRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            5,
            "INSERT INTO near.near_hos_staging_receipt_status (
                receipt_id,
                transaction_hash,
                executed,
                first_seen_height,
                last_seen_height
            ) VALUES ",
            "
            ON CONFLICT (receipt_id) DO UPDATE SET
                transaction_hash = EXCLUDED.transaction_hash,
                executed = near.near_hos_staging_receipt_status.executed OR EXCLUDED.executed,
                last_seen_height = GREATEST(
                    near.near_hos_staging_receipt_status.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            |row, values| {
                values.push(row.receipt_id.clone().into());
                values.push(row.transaction_hash.clone().into());
                values.push(row.executed.into());
                values.push(row.block_height.into());
                values.push(row.block_height.into());
            },
            "failed to bulk upsert near_hos_staging_receipt_status",
        )
        .await
    }

    async fn flush_block_writes(
        db: &impl ConnectionTrait,
        writes: HosBlockWrites,
        stats: &mut StageMetrics,
    ) -> Result<()> {
        let total_write_rows = writes.tx_states.len()
            + writes.receipt_tx_hash_map.len()
            + writes.staging_blocks.len()
            + writes.staging_chunks.len()
            + writes.staging_state_changes.len()
            + writes.staging_transactions.len()
            + writes.staging_receipts.len()
            + writes.staging_execution_outcomes.len()
            + writes.staging_receipt_actions.len()
            + writes.staging_receipt_edges.len()
            + writes.staging_receipt_statuses.len();
        stats.record_count("staging_write_rows", total_write_rows);

        let tx_states = writes.tx_states.into_values().collect::<Vec<_>>();
        let receipt_tx_hash_map = writes.receipt_tx_hash_map.into_values().collect::<Vec<_>>();
        let staging_blocks = writes.staging_blocks.into_values().collect::<Vec<_>>();
        let staging_chunks = writes.staging_chunks.into_values().collect::<Vec<_>>();
        let staging_state_changes = writes
            .staging_state_changes
            .into_values()
            .collect::<Vec<_>>();
        let staging_transactions = writes
            .staging_transactions
            .into_values()
            .collect::<Vec<_>>();
        let staging_receipts = writes.staging_receipts.into_values().collect::<Vec<_>>();
        let staging_execution_outcomes = writes
            .staging_execution_outcomes
            .into_values()
            .collect::<Vec<_>>();
        let staging_receipt_actions = writes
            .staging_receipt_actions
            .into_values()
            .collect::<Vec<_>>();
        let staging_receipt_edges = writes
            .staging_receipt_edges
            .into_values()
            .collect::<Vec<_>>();
        let staging_receipt_statuses = writes
            .staging_receipt_statuses
            .into_values()
            .collect::<Vec<_>>();

        let start = Instant::now();
        Self::flush_tx_state_upserts(db, &tx_states).await?;
        stats.record_phase_elapsed("tx_state_flush", start);

        let start = Instant::now();
        Self::flush_receipt_tx_hash_map_upserts(db, &receipt_tx_hash_map).await?;
        stats.record_phase_elapsed("receipt_map_flush", start);

        let start = Instant::now();
        Self::flush_staging_block_upserts(db, &staging_blocks).await?;
        stats.record_phase_elapsed("block_flush", start);

        let start = Instant::now();
        Self::flush_staging_transaction_upserts(db, &staging_transactions).await?;
        stats.record_phase_elapsed("transaction_flush", start);

        let start = Instant::now();
        Self::flush_staging_receipt_upserts(db, &staging_receipts).await?;
        stats.record_phase_elapsed("receipt_flush", start);

        let start = Instant::now();
        Self::flush_staging_execution_outcome_upserts(db, &staging_execution_outcomes).await?;
        stats.record_phase_elapsed("execution_outcome_flush", start);

        let start = Instant::now();
        Self::flush_staging_receipt_action_upserts(db, &staging_receipt_actions).await?;
        stats.record_phase_elapsed("receipt_action_flush", start);

        let start = Instant::now();
        Self::flush_staging_receipt_edge_upserts(db, &staging_receipt_edges).await?;
        stats.record_phase_elapsed("receipt_edge_flush", start);

        let start = Instant::now();
        Self::flush_staging_receipt_status_upserts(db, &staging_receipt_statuses).await?;
        stats.record_phase_elapsed("receipt_status_flush", start);

        let start = Instant::now();
        Self::flush_staging_chunk_upserts(db, &staging_chunks).await?;
        stats.record_phase_elapsed("chunk_flush", start);

        let start = Instant::now();
        Self::flush_staging_state_changes_upserts(db, &staging_state_changes).await?;
        stats.record_phase_elapsed("state_changes_flush", start);

        Ok(())
    }

    async fn refresh_pending_counts_for_txs(
        db: &impl ConnectionTrait,
        tx_hashes: &[String],
    ) -> Result<()> {
        if tx_hashes.is_empty() {
            return Ok(());
        }

        let mut placeholders = String::new();
        let mut values = Vec::new();
        for (idx, tx_hash) in tx_hashes.iter().enumerate() {
            if idx > 0 {
                placeholders.push_str(", ");
            }
            placeholders.push_str(&format!("${}", idx + 1));
            values.push(tx_hash.clone().into());
        }
        let pending_status_placeholder = tx_hashes.len() + 1;
        values.push(TX_STATUS_PENDING.into());

        let sql = format!(
            "UPDATE near.near_hos_tx_state ts
             SET pending_receipt_count = COALESCE((
                     SELECT COUNT(*)::BIGINT
                     FROM near.near_hos_staging_receipt_status rs
                     WHERE rs.transaction_hash = ts.transaction_hash
                       AND rs.executed = FALSE
                 ), 0),
                 updated_at = NOW()
             WHERE ts.transaction_hash IN ({placeholders})
               AND ts.status = ${pending_status_placeholder}"
        );

        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            values,
        ))
        .await
        .context("failed to refresh near_hos_tx_state pending counts")?;

        Ok(())
    }

    async fn load_finalize_candidates(
        db: &impl ConnectionTrait,
    ) -> Result<Vec<(String, bool, i64, i64)>> {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT transaction_hash, has_hos_touch, pending_receipt_count, first_seen_height
                 FROM near.near_hos_tx_state
                 WHERE status = $1
                   AND transaction_hash IS NOT NULL
                   AND pending_receipt_count = 0
                 ORDER BY first_seen_height ASC
                 LIMIT 5000",
                vec![TX_STATUS_PENDING.into()],
            ))
            .await
            .context("failed to load finalize candidates from near_hos_tx_state")?;

        let mut candidates = Vec::new();
        for row in rows {
            let tx_hash: String = row
                .try_get("", "transaction_hash")
                .context("failed to read near_hos_tx_state.transaction_hash")?;
            let has_hos_touch: bool = row
                .try_get("", "has_hos_touch")
                .context("failed to read near_hos_tx_state.has_hos_touch")?;
            let pending_receipt_count: i64 = row
                .try_get("", "pending_receipt_count")
                .context("failed to read near_hos_tx_state.pending_receipt_count")?;
            let first_seen_height: i64 = row
                .try_get("", "first_seen_height")
                .context("failed to read near_hos_tx_state.first_seen_height")?;

            candidates.push((
                tx_hash,
                has_hos_touch,
                pending_receipt_count,
                first_seen_height,
            ));
        }

        Ok(candidates)
    }

    async fn staging_transaction_exists(
        db: &impl ConnectionTrait,
        transaction_hash: &str,
    ) -> Result<bool> {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT 1
                 FROM near.near_hos_staging_transactions
                 WHERE hash = $1
                 LIMIT 1",
                vec![transaction_hash.into()],
            ))
            .await
            .context("failed to check staged transaction existence")?;

        Ok(row.is_some())
    }

    async fn mark_tx_state_finished(
        db: &impl ConnectionTrait,
        transaction_hash: &str,
        status: &str,
        pending_receipt_count: i64,
        incomplete: bool,
        error_reason: Option<&str>,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE near.near_hos_tx_state
             SET status = $2,
                 pending_receipt_count = $3,
                 finalized_at = NOW(),
                 incomplete = $4,
                 error_reason = COALESCE($5, error_reason),
                 updated_at = NOW()
             WHERE graph_key = $1",
            vec![
                transaction_hash.into(),
                status.into(),
                pending_receipt_count.into(),
                incomplete.into(),
                error_reason.map(ToString::to_string).into(),
            ],
        ))
        .await
        .context("failed to update near_hos_tx_state final status")?;

        Ok(())
    }

    async fn mark_tx_state_incomplete(
        db: &impl ConnectionTrait,
        transaction_hash: &str,
        pending_receipt_count: i64,
        error_reason: &str,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE near.near_hos_tx_state
             SET status = $2,
                 pending_receipt_count = $3,
                 finalized_at = NULL,
                 incomplete = TRUE,
                 error_reason = $4,
                 updated_at = NOW()
             WHERE graph_key = $1",
            vec![
                transaction_hash.into(),
                TX_STATUS_INCOMPLETE_MISSING_TX_HASH.into(),
                pending_receipt_count.into(),
                error_reason.into(),
            ],
        ))
        .await
        .context("failed to mark near_hos_tx_state incomplete")?;

        Ok(())
    }

    async fn purge_staging_for_tx(db: &impl ConnectionTrait, transaction_hash: &str) -> Result<()> {
        let statements: &[&str] = &[
            "DELETE FROM near.near_hos_staging_blocks WHERE transaction_hash = $1",
            "DELETE FROM near.near_hos_staging_chunks WHERE transaction_hash = $1",
            "DELETE FROM near.near_hos_staging_state_changes WHERE transaction_hash = $1",
            "DELETE FROM near.near_hos_staging_receipt_actions WHERE transaction_hash = $1",
            "DELETE FROM near.near_hos_staging_receipt_edges WHERE transaction_hash = $1",
            "DELETE FROM near.near_hos_staging_execution_outcomes WHERE transaction_hash = $1",
            "DELETE FROM near.near_hos_staging_receipts WHERE transaction_hash = $1",
            "DELETE FROM near.near_hos_staging_receipt_status WHERE transaction_hash = $1",
            "DELETE FROM near.near_hos_staging_transactions WHERE hash = $1",
        ];

        for statement in statements {
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                *statement,
                vec![transaction_hash.into()],
            ))
            .await
            .with_context(|| format!("failed purging staging rows for tx {transaction_hash}"))?;
        }

        Ok(())
    }

    async fn promote_staged_tx_graph(
        db: &impl ConnectionTrait,
        transaction_hash: &str,
    ) -> Result<()> {
        let statements: &[&str] = &[
            "INSERT INTO near.near_transactions (
                hash, block_height, signer_id, receiver_id, nonce, priority_fee,
                public_key, signature, actions, raw_data, shard_id
            )
            SELECT
                hash, block_height, signer_id, receiver_id, nonce, priority_fee,
                public_key, signature, actions, raw_data, shard_id
            FROM near.near_hos_staging_transactions
            WHERE hash = $1
            ON CONFLICT (hash) DO UPDATE SET
                block_height = EXCLUDED.block_height,
                signer_id = EXCLUDED.signer_id,
                receiver_id = EXCLUDED.receiver_id,
                nonce = EXCLUDED.nonce,
                priority_fee = EXCLUDED.priority_fee,
                public_key = EXCLUDED.public_key,
                signature = EXCLUDED.signature,
                actions = EXCLUDED.actions,
                raw_data = COALESCE(EXCLUDED.raw_data, near.near_transactions.raw_data),
                shard_id = EXCLUDED.shard_id",
            "INSERT INTO near.near_receipts (
                receipt_id, block_height, shard_id, predecessor_id, receiver_id, receipt, raw_data
            )
            SELECT
                receipt_id, block_height, shard_id, predecessor_id, receiver_id, receipt, raw_data
            FROM near.near_hos_staging_receipts
            WHERE transaction_hash = $1
            ON CONFLICT (receipt_id) DO UPDATE SET
                block_height = EXCLUDED.block_height,
                shard_id = EXCLUDED.shard_id,
                predecessor_id = EXCLUDED.predecessor_id,
                receiver_id = EXCLUDED.receiver_id,
                receipt = EXCLUDED.receipt,
                raw_data = COALESCE(near.near_receipts.raw_data, EXCLUDED.raw_data)",
            "INSERT INTO near.near_execution_outcomes (
                id, block_height, shard_id, receipt_id, outcome, raw_data,
                transaction_hash, block_hash, chunk_hash, gas_burnt, gas_used,
                tokens_burnt, executor_account_id, status, outcome_receipt_ids,
                executed_in_block_hash, logs, results_json, block_timestamp
            )
            SELECT
                id, block_height, shard_id, receipt_id, outcome, raw_data,
                transaction_hash, block_hash, chunk_hash, gas_burnt, gas_used,
                tokens_burnt, executor_account_id, status, outcome_receipt_ids,
                executed_in_block_hash, logs, results_json, block_timestamp
            FROM near.near_hos_staging_execution_outcomes
            WHERE transaction_hash = $1
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
            "INSERT INTO near.near_receipt_actions (
                id, block_height, receipt_id, execution_outcome_id, transaction_hash, shard_id,
                signer_account_id, signer_public_key, gas_price, action_kind, action_json,
                predecessor_id, receiver_id, block_hash, chunk_hash, author, method_name, gas,
                deposit, args_base64, args_json, action_index, block_timestamp
            )
            SELECT
                id, block_height, receipt_id, execution_outcome_id, transaction_hash, shard_id,
                signer_account_id, signer_public_key, gas_price, action_kind, action_json,
                predecessor_id, receiver_id, block_hash, chunk_hash, author, method_name, gas,
                deposit, args_base64, args_json, action_index, block_timestamp
            FROM near.near_hos_staging_receipt_actions
            WHERE transaction_hash = $1
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
            "INSERT INTO near.near_receipt_edges (
                parent_receipt_id, child_receipt_id, parent_execution_outcome_id,
                block_height, block_hash, block_timestamp, transaction_hash
            )
            SELECT
                parent_receipt_id, child_receipt_id, parent_execution_outcome_id,
                block_height, block_hash, block_timestamp, transaction_hash
            FROM near.near_hos_staging_receipt_edges
            WHERE transaction_hash = $1
            ON CONFLICT (parent_receipt_id, child_receipt_id) DO UPDATE SET
                parent_execution_outcome_id = EXCLUDED.parent_execution_outcome_id,
                block_height = EXCLUDED.block_height,
                block_hash = EXCLUDED.block_hash,
                block_timestamp = EXCLUDED.block_timestamp,
                transaction_hash = COALESCE(
                    near.near_receipt_edges.transaction_hash,
                    EXCLUDED.transaction_hash
                )",
            "INSERT INTO near.near_blocks (
                height, hash, prev_hash, epoch_id, next_epoch_id, author,
                timestamp, timestamp_nanosec, gas_price, total_supply,
                validator_reward, protocol_version, chunks_included,
                block_ordinal, prev_height, raw_data
            )
            SELECT DISTINCT
                height, hash, prev_hash, epoch_id, next_epoch_id, author,
                timestamp, timestamp_nanosec, gas_price, total_supply,
                validator_reward, protocol_version, chunks_included,
                block_ordinal, prev_height, raw_data
            FROM near.near_hos_staging_blocks
            WHERE transaction_hash = $1
            ON CONFLICT (height) DO NOTHING",
            "INSERT INTO near.near_chunks (
                chunk_hash, block_height, shard_id, author, height_created,
                height_included, gas_used, gas_limit, validator_reward, raw_data
            )
            SELECT DISTINCT
                chunk_hash, block_height, shard_id, author, height_created,
                height_included, gas_used, gas_limit, validator_reward, raw_data
            FROM near.near_hos_staging_chunks
            WHERE transaction_hash = $1
            ON CONFLICT (chunk_hash) DO NOTHING",
            "INSERT INTO near.near_state_changes (
                block_height, shard_id, state_changes
            )
            SELECT DISTINCT
                block_height, shard_id, state_changes
            FROM near.near_hos_staging_state_changes
            WHERE transaction_hash = $1
            ON CONFLICT (block_height, shard_id) DO NOTHING",
        ];

        for statement in statements {
            db.execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                *statement,
                vec![transaction_hash.into()],
            ))
            .await
            .with_context(|| format!("failed promoting staged tx graph {transaction_hash}"))?;
        }

        Ok(())
    }

    async fn run_receipt_tx_map_cleanup(
        &self,
        db: &impl ConnectionTrait,
        block_height: i64,
    ) -> Result<()> {
        if self.raw_enabled
            || self.receipt_tx_map_cleanup_interval_blocks <= 0
            || self.receipt_tx_map_cleanup_batch_size <= 0
        {
            return Ok(());
        }

        if block_height % self.receipt_tx_map_cleanup_interval_blocks != 0 {
            return Ok(());
        }

        let cutoff_height =
            block_height.saturating_sub(self.receipt_tx_map_retention_blocks.max(0));
        let batch_size = self.receipt_tx_map_cleanup_batch_size.max(1);
        let max_batches = self.receipt_tx_map_cleanup_max_batches_per_run.max(1);

        for _ in 0..max_batches {
            let result = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "WITH deletable AS (
                        SELECT m.receipt_id
                        FROM near.near_receipt_tx_hash_map m
                        WHERE m.last_seen_height < $1
                          AND NOT EXISTS (
                                SELECT 1
                                FROM near.near_transactions nt
                                WHERE nt.hash = m.transaction_hash
                          )
                          AND NOT EXISTS (
                                SELECT 1
                                FROM near.near_hos_staging_transactions st
                                WHERE st.hash = m.transaction_hash
                          )
                          AND NOT EXISTS (
                                SELECT 1
                                FROM near.near_hos_staging_receipt_status rs
                                WHERE rs.transaction_hash = m.transaction_hash
                          )
                          AND NOT EXISTS (
                                SELECT 1
                                FROM near.near_hos_tx_state ts
                                WHERE ts.transaction_hash = m.transaction_hash
                                  AND ts.status IN ($3, $4)
                          )
                        ORDER BY m.last_seen_height ASC
                        LIMIT $2
                     )
                     DELETE FROM near.near_receipt_tx_hash_map m
                     USING deletable d
                     WHERE m.receipt_id = d.receipt_id",
                    vec![
                        cutoff_height.into(),
                        batch_size.into(),
                        TX_STATUS_PENDING.into(),
                        TX_STATUS_INCOMPLETE_MISSING_TX_HASH.into(),
                    ],
                ))
                .await
                .context("failed running near_receipt_tx_hash_map cleanup")?;

            if result.rows_affected() == 0 {
                break;
            }
        }

        Ok(())
    }

    async fn warn_on_stalled_pending_graphs(
        &self,
        db: &impl ConnectionTrait,
        block_height: i64,
    ) -> Result<()> {
        let warning_threshold_height =
            block_height.saturating_sub(STALLED_PENDING_GRAPH_WARNING_BLOCKS.max(0));
        let limit = self.staging_gc_batch_size.clamp(1, 100);
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT transaction_hash, has_hos_touch, pending_receipt_count, last_seen_height
                 FROM near.near_hos_tx_state
                 WHERE status = $1
                   AND transaction_hash IS NOT NULL
                   AND pending_receipt_count > 0
                   AND last_seen_height <= $2
                 ORDER BY last_seen_height ASC
                 LIMIT $3",
                vec![
                    TX_STATUS_PENDING.into(),
                    warning_threshold_height.into(),
                    limit.into(),
                ],
            ))
            .await
            .context("failed to load stalled pending near_hos_tx_state rows")?;

        for row in rows {
            let transaction_hash: String = row
                .try_get("", "transaction_hash")
                .context("failed to read stalled near_hos_tx_state.transaction_hash")?;
            let has_hos_touch: bool = row
                .try_get("", "has_hos_touch")
                .context("failed to read stalled near_hos_tx_state.has_hos_touch")?;
            let pending_receipt_count: i64 = row
                .try_get("", "pending_receipt_count")
                .context("failed to read stalled near_hos_tx_state.pending_receipt_count")?;
            let last_seen_height: i64 = row
                .try_get("", "last_seen_height")
                .context("failed to read stalled near_hos_tx_state.last_seen_height")?;

            warn!(
                module = MODULE_NAME,
                tx_hash = %transaction_hash,
                pending_receipt_count,
                has_hos_touch,
                last_seen_height,
                age_blocks = block_height.saturating_sub(last_seen_height),
                "Pending tx graph remains unresolved; preserving staging rows"
            );
        }

        Ok(())
    }

    async fn run_staging_gc(&self, db: &impl ConnectionTrait, block_height: i64) -> Result<()> {
        self.run_receipt_tx_map_cleanup(db, block_height).await?;

        if self.staging_gc_interval_blocks <= 0 || self.staging_gc_batch_size <= 0 {
            return Ok(());
        }

        if block_height % self.staging_gc_interval_blocks != 0 {
            return Ok(());
        }

        self.warn_on_stalled_pending_graphs(db, block_height)
            .await?;

        let cutoff_height = block_height.saturating_sub(TX_STATE_GC_RETENTION_BLOCKS.max(0));
        let batch_size = self.staging_gc_batch_size.max(1);
        let max_batches = self.staging_gc_max_batches_per_run.max(1);

        for _ in 0..max_batches {
            let result = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "WITH deletable AS (
                        SELECT graph_key
                        FROM near.near_hos_tx_state
                        WHERE status IN ($1, $2)
                          AND last_seen_height < $3
                        ORDER BY last_seen_height ASC
                        LIMIT $4
                     )
                     DELETE FROM near.near_hos_tx_state ts
                     USING deletable d
                     WHERE ts.graph_key = d.graph_key",
                    vec![
                        TX_STATUS_FINALIZED.into(),
                        TX_STATUS_PRUNED_NON_HOS.into(),
                        cutoff_height.into(),
                        batch_size.into(),
                    ],
                ))
                .await
                .context("failed running near_hos_tx_state GC")?;

            if result.rows_affected() == 0 {
                break;
            }
        }

        Ok(())
    }

    #[instrument(skip(self, msg, db), level = "debug", fields(height = %msg.block.header.height))]
    async fn store_hos_projection(
        &self,
        msg: &StreamerMessage,
        db: &impl ConnectionTrait,
    ) -> Result<StageMetrics> {
        let mut stats = StageMetrics::new();
        let block_height = msg.block.header.height as i64;
        let block_hash = msg.block.header.hash.to_string();
        let block_timestamp = msg.block.header.timestamp as i64;
        let default_author = msg.block.author.to_string();
        let block_prev_hash = msg.block.header.prev_hash.to_string();
        let block_epoch_id = msg.block.header.epoch_id.to_string();
        let block_next_epoch_id = msg.block.header.next_epoch_id.to_string();
        let block_gas_price = msg.block.header.gas_price.to_string();
        let block_total_supply = msg.block.header.total_supply.to_string();
        let block_validator_reward = msg.block.header.validator_reward.to_string();
        let block_protocol_version = msg.block.header.latest_protocol_version as i32;
        let block_chunks_included = msg.block.header.chunks_included as i64;
        let block_block_ordinal = msg.block.header.block_ordinal.map(|value| value as i64);
        let block_prev_height = msg.block.header.prev_height.map(|value| value as i64);
        let block_timestamp_nanosec = msg.block.header.timestamp_nanosec as i64;
        let block_raw_data = Self::serialize_json(&msg.block, "staging block raw_data")?;

        let mut receipt_tx_hashes = build_receipt_tx_hashes(&msg.shards);
        let mut touched_tx_hashes: HashSet<String> = HashSet::new();
        let mut lookup_receipt_ids: Vec<String> = Vec::new();

        for shard in &msg.shards {
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        lookup_receipt_ids.push(receipt.receipt_id.to_string());
                    }
                    for child_receipt_id in &tx_with_outcome
                        .outcome
                        .execution_outcome
                        .outcome
                        .receipt_ids
                    {
                        lookup_receipt_ids.push(child_receipt_id.to_string());
                    }
                }

                for receipt in &chunk.receipts {
                    lookup_receipt_ids.push(receipt.receipt_id.to_string());
                }

                for receipt in &chunk.local_receipts {
                    lookup_receipt_ids.push(receipt.receipt_id.to_string());
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                lookup_receipt_ids.push(outcome_with_receipt.receipt.receipt_id.to_string());
                for child_receipt_id in &outcome_with_receipt.execution_outcome.outcome.receipt_ids
                {
                    lookup_receipt_ids.push(child_receipt_id.to_string());
                }
            }
        }

        stats.record_count("receipt_lookup_count", lookup_receipt_ids.len());
        let lineage_lookup_start = Instant::now();
        let existing_staging_status =
            Self::load_receipt_tx_from_staging_status(db, &lookup_receipt_ids).await?;
        for (receipt_id, transaction_hash) in existing_staging_status {
            receipt_tx_hashes
                .entry(receipt_id)
                .or_insert_with(|| transaction_hash);
        }

        let mapped_receipts = load_receipt_tx_hash_map(db, &lookup_receipt_ids).await?;
        for (receipt_id, transaction_hash) in mapped_receipts {
            receipt_tx_hashes
                .entry(receipt_id)
                .or_insert_with(|| transaction_hash);
        }
        stats.record_phase_elapsed("lineage_lookup", lineage_lookup_start);

        let staging_write_start = Instant::now();
        let mut writes = HosBlockWrites::default();
        let note_block_for_tx = |writes: &mut HosBlockWrites, transaction_hash: &str| {
            writes.note_staging_block(StagingBlockRow {
                transaction_hash: transaction_hash.to_string(),
                height: block_height,
                hash: block_hash.clone(),
                prev_hash: block_prev_hash.clone(),
                epoch_id: block_epoch_id.clone(),
                next_epoch_id: block_next_epoch_id.clone(),
                author: default_author.clone(),
                timestamp: block_timestamp,
                timestamp_nanosec: block_timestamp_nanosec,
                gas_price: block_gas_price.clone(),
                total_supply: block_total_supply.clone(),
                validator_reward: block_validator_reward.clone(),
                protocol_version: block_protocol_version,
                chunks_included: block_chunks_included,
                block_ordinal: block_block_ordinal,
                prev_height: block_prev_height,
                raw_data: block_raw_data.clone(),
                seen_height: block_height,
            });
        };

        for shard in &msg.shards {
            let mut shard_touched_tx_hashes: HashSet<String> = HashSet::new();
            let chunk_hash = shard
                .chunk
                .as_ref()
                .map(|chunk| chunk.header.chunk_hash.to_string())
                .unwrap_or_default();
            let chunk_author = shard
                .chunk
                .as_ref()
                .map(|chunk| chunk.author.to_string())
                .unwrap_or_else(|| default_author.clone());
            let shard_id = Self::shard_to_i64(shard.shard_id);

            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    let tx_hash = tx_with_outcome.transaction.hash.to_string();
                    touched_tx_hashes.insert(tx_hash.clone());
                    shard_touched_tx_hashes.insert(tx_hash.clone());

                    let tx_root_related = self.is_hos_related_with_signer(
                        tx_with_outcome.transaction.receiver_id.as_ref(),
                        tx_with_outcome.transaction.signer_id.as_ref(),
                        Some(tx_with_outcome.transaction.signer_id.as_ref()),
                    );
                    writes.note_tx_state(&tx_hash, block_height, tx_root_related);
                    note_block_for_tx(&mut writes, &tx_hash);
                    writes.note_staging_transaction(StagingTransactionRow {
                        hash: tx_hash.clone(),
                        block_height,
                        shard_id,
                        signer_id: tx_with_outcome.transaction.signer_id.to_string(),
                        receiver_id: tx_with_outcome.transaction.receiver_id.to_string(),
                        nonce: tx_with_outcome.transaction.nonce as i64,
                        priority_fee: tx_with_outcome.transaction.priority_fee as i64,
                        public_key: tx_with_outcome.transaction.public_key.to_string(),
                        signature: tx_with_outcome.transaction.signature.to_string(),
                        actions: Self::serialize_json(
                            &tx_with_outcome.transaction.actions,
                            "HoS transaction actions",
                        )?,
                        raw_data: Self::serialize_json(
                            &tx_with_outcome.transaction,
                            "HoS transaction raw_data",
                        )?,
                    });

                    let tx_outcome_receipt_id = tx_with_outcome
                        .outcome
                        .receipt
                        .as_ref()
                        .map(|receipt| receipt.receipt_id.to_string());
                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        let receipt_id = receipt.receipt_id.to_string();
                        receipt_tx_hashes.insert(receipt_id.clone(), tx_hash.clone());
                        writes.note_receipt_tx_hash_mapping(&receipt_id, &tx_hash, block_height);

                        let is_hos_root_related = self.is_hos_related_receipt(receipt);
                        if is_hos_root_related {
                            writes.note_tx_state(&tx_hash, block_height, true);
                        }

                        writes.note_staging_receipt(StagingReceiptRow {
                            receipt_id: receipt_id.clone(),
                            transaction_hash: tx_hash.clone(),
                            block_height,
                            shard_id,
                            predecessor_id: receipt.predecessor_id.to_string(),
                            receiver_id: receipt.receiver_id.to_string(),
                            receipt: Self::serialize_json(&receipt.receipt, "receipt body")?,
                            raw_data: Self::serialize_json(receipt, "receipt row")?,
                        });
                        writes.note_staging_receipt_status(StagingReceiptStatusRow {
                            receipt_id,
                            transaction_hash: tx_hash.clone(),
                            executed: true,
                            block_height,
                        });
                    }

                    let tx_outcome_view = &tx_with_outcome.outcome.execution_outcome;
                    let tx_outcome = &tx_outcome_view.outcome;
                    let tx_outcome_id = tx_outcome_view.id.to_string();
                    let tx_outcome_receipt_ids = JsonValue::Array(
                        tx_outcome
                            .receipt_ids
                            .iter()
                            .map(|value| JsonValue::String(value.to_string()))
                            .collect(),
                    );
                    let tx_logs = JsonValue::Array(
                        tx_outcome
                            .logs
                            .iter()
                            .cloned()
                            .map(JsonValue::String)
                            .collect(),
                    );
                    let (tx_status, tx_results_json) = Self::extract_status(&tx_outcome.status);
                    writes.note_staging_execution_outcome(StagingExecutionOutcomeRow {
                        id: tx_outcome_id.clone(),
                        transaction_hash: tx_hash.clone(),
                        block_height,
                        shard_id,
                        receipt_id: tx_outcome_receipt_id.clone(),
                        outcome: Self::serialize_json(
                            &tx_outcome_view.outcome,
                            "tx execution outcome",
                        )?,
                        raw_data: Self::serialize_json(
                            tx_outcome_view,
                            "tx execution outcome row",
                        )?,
                        block_hash: block_hash.clone(),
                        chunk_hash: chunk_hash.clone(),
                        gas_burnt: tx_outcome.gas_burnt.as_gas() as i64,
                        gas_used: tx_outcome.gas_burnt.as_gas() as f64,
                        tokens_burnt: tx_outcome.tokens_burnt.as_yoctonear() as f64 / 1e24,
                        executor_account_id: tx_outcome.executor_id.to_string(),
                        status: tx_status.to_string(),
                        outcome_receipt_ids: tx_outcome_receipt_ids,
                        executed_in_block_hash: block_hash.clone(),
                        logs: tx_logs,
                        results_json: tx_results_json,
                        block_timestamp,
                    });

                    if let Some(parent_receipt_id) = tx_outcome_receipt_id.as_deref() {
                        for child_receipt_id in &tx_outcome.receipt_ids {
                            let child_receipt_id = child_receipt_id.to_string();
                            receipt_tx_hashes.insert(child_receipt_id.clone(), tx_hash.clone());
                            writes.note_receipt_tx_hash_mapping(
                                &child_receipt_id,
                                &tx_hash,
                                block_height,
                            );
                            writes.note_staging_receipt_edge(StagingReceiptEdgeRow {
                                parent_receipt_id: parent_receipt_id.to_string(),
                                child_receipt_id: child_receipt_id.clone(),
                                transaction_hash: tx_hash.clone(),
                                parent_execution_outcome_id: tx_outcome_id.clone(),
                                block_height,
                                block_hash: block_hash.clone(),
                                block_timestamp,
                            });
                            writes.note_staging_receipt_status(StagingReceiptStatusRow {
                                receipt_id: child_receipt_id,
                                transaction_hash: tx_hash.clone(),
                                executed: false,
                                block_height,
                            });
                        }
                    } else {
                        for child_receipt_id in &tx_outcome.receipt_ids {
                            let child_receipt_id = child_receipt_id.to_string();
                            receipt_tx_hashes.insert(child_receipt_id.clone(), tx_hash.clone());
                            writes.note_receipt_tx_hash_mapping(
                                &child_receipt_id,
                                &tx_hash,
                                block_height,
                            );
                            writes.note_staging_receipt_status(StagingReceiptStatusRow {
                                receipt_id: child_receipt_id,
                                transaction_hash: tx_hash.clone(),
                                executed: false,
                                block_height,
                            });
                        }
                    }
                }
            }

            if let Some(chunk) = &shard.chunk {
                for receipt in chunk.receipts.iter().chain(chunk.local_receipts.iter()) {
                    let receipt_id = receipt.receipt_id.to_string();
                    let transaction_hash = receipt_tx_hashes.get(&receipt_id).cloned();
                    let is_hos_root_related = self.is_hos_related_receipt(receipt);

                    let Some(transaction_hash) = transaction_hash else {
                        if is_hos_root_related {
                            error!(
                                module = MODULE_NAME,
                                receipt_id = %receipt_id,
                                height = block_height,
                                "HoS observed receipt missing transaction hash; marking incomplete"
                            );
                            Self::mark_incomplete_missing_tx_hash(db, &receipt_id, block_height)
                                .await?;
                        }
                        continue;
                    };

                    touched_tx_hashes.insert(transaction_hash.clone());
                    shard_touched_tx_hashes.insert(transaction_hash.clone());
                    writes.note_tx_state(&transaction_hash, block_height, is_hos_root_related);
                    writes.note_receipt_tx_hash_mapping(
                        &receipt_id,
                        &transaction_hash,
                        block_height,
                    );
                    note_block_for_tx(&mut writes, &transaction_hash);
                    writes.note_staging_receipt(StagingReceiptRow {
                        receipt_id: receipt_id.clone(),
                        transaction_hash: transaction_hash.clone(),
                        block_height,
                        shard_id,
                        predecessor_id: receipt.predecessor_id.to_string(),
                        receiver_id: receipt.receiver_id.to_string(),
                        receipt: Self::serialize_json(&receipt.receipt, "observed receipt body")?,
                        raw_data: Self::serialize_json(receipt, "observed receipt row")?,
                    });
                    writes.note_staging_receipt_status(StagingReceiptStatusRow {
                        receipt_id,
                        transaction_hash,
                        executed: false,
                        block_height,
                    });
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                let receipt_id = receipt.receipt_id.to_string();
                let transaction_hash = receipt_tx_hashes.get(&receipt_id).cloned();
                let is_hos_root_related = self.is_hos_related_receipt(receipt);

                let Some(transaction_hash) = transaction_hash else {
                    if is_hos_root_related {
                        error!(
                            module = MODULE_NAME,
                            receipt_id = %receipt_id,
                            height = block_height,
                            "HoS executed receipt missing transaction hash; marking incomplete"
                        );
                        Self::mark_incomplete_missing_tx_hash(db, &receipt_id, block_height)
                            .await?;
                    }
                    continue;
                };

                receipt_tx_hashes.insert(receipt_id.clone(), transaction_hash.clone());
                touched_tx_hashes.insert(transaction_hash.clone());
                shard_touched_tx_hashes.insert(transaction_hash.clone());
                writes.note_tx_state(&transaction_hash, block_height, is_hos_root_related);
                writes.note_receipt_tx_hash_mapping(&receipt_id, &transaction_hash, block_height);
                note_block_for_tx(&mut writes, &transaction_hash);

                writes.note_staging_receipt(StagingReceiptRow {
                    receipt_id: receipt_id.clone(),
                    transaction_hash: transaction_hash.clone(),
                    block_height,
                    shard_id,
                    predecessor_id: receipt.predecessor_id.to_string(),
                    receiver_id: receipt.receiver_id.to_string(),
                    receipt: Self::serialize_json(&receipt.receipt, "receipt body")?,
                    raw_data: Self::serialize_json(receipt, "receipt row")?,
                });
                writes.note_staging_receipt_status(StagingReceiptStatusRow {
                    receipt_id: receipt_id.clone(),
                    transaction_hash: transaction_hash.clone(),
                    executed: true,
                    block_height,
                });

                let outcome_view = &outcome_with_receipt.execution_outcome;
                let outcome = &outcome_view.outcome;
                let outcome_id = outcome_view.id.to_string();
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

                writes.note_staging_execution_outcome(StagingExecutionOutcomeRow {
                    id: outcome_id.clone(),
                    transaction_hash: transaction_hash.clone(),
                    block_height,
                    shard_id,
                    receipt_id: Some(receipt_id.clone()),
                    outcome: Self::serialize_json(&outcome_view.outcome, "execution outcome")?,
                    raw_data: Self::serialize_json(outcome_view, "execution outcome row")?,
                    block_hash: block_hash.clone(),
                    chunk_hash: chunk_hash.clone(),
                    gas_burnt: outcome.gas_burnt.as_gas() as i64,
                    gas_used: outcome.gas_burnt.as_gas() as f64,
                    tokens_burnt: outcome.tokens_burnt.as_yoctonear() as f64 / 1e24,
                    executor_account_id,
                    status: status.to_string(),
                    outcome_receipt_ids: outcome_receipt_ids.clone(),
                    executed_in_block_hash: block_hash.clone(),
                    logs,
                    results_json,
                    block_timestamp,
                });

                for child_receipt_id in &outcome.receipt_ids {
                    let child_receipt_id = child_receipt_id.to_string();
                    receipt_tx_hashes.insert(child_receipt_id.clone(), transaction_hash.clone());
                    writes.note_receipt_tx_hash_mapping(
                        &child_receipt_id,
                        &transaction_hash,
                        block_height,
                    );
                    writes.note_staging_receipt_edge(StagingReceiptEdgeRow {
                        parent_receipt_id: receipt_id.clone(),
                        child_receipt_id: child_receipt_id.clone(),
                        transaction_hash: transaction_hash.clone(),
                        parent_execution_outcome_id: outcome_id.clone(),
                        block_height,
                        block_hash: block_hash.clone(),
                        block_timestamp,
                    });
                    writes.note_staging_receipt_status(StagingReceiptStatusRow {
                        receipt_id: child_receipt_id,
                        transaction_hash: transaction_hash.clone(),
                        executed: false,
                        block_height,
                    });
                }

                if let ReceiptEnumView::Action {
                    signer_id,
                    signer_public_key,
                    gas_price,
                    actions,
                    ..
                } = &receipt.receipt
                {
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

                        writes.note_staging_receipt_action(StagingReceiptActionRow {
                            id: format!("{receipt_id}-{action_index}"),
                            transaction_hash: transaction_hash.clone(),
                            block_height,
                            receipt_id: receipt_id.clone(),
                            execution_outcome_id: outcome_id.clone(),
                            shard_id,
                            signer_account_id: Some(signer_id_value),
                            signer_public_key: Some(signer_public_key_value),
                            gas_price: Some(gas_price_value),
                            action_kind,
                            action_json,
                            predecessor_id: receipt.predecessor_id.to_string(),
                            receiver_id: receipt.receiver_id.to_string(),
                            block_hash: block_hash.clone(),
                            chunk_hash: chunk_hash.clone(),
                            author: chunk_author.clone(),
                            method_name,
                            gas,
                            deposit,
                            args_base64,
                            args_json,
                            action_index: action_index as i32,
                            block_timestamp,
                        });
                    }
                }
            }

            if let Some(chunk) = &shard.chunk {
                let header = &chunk.header;
                let chunk_raw_data = Self::serialize_json(chunk, "chunk raw_data")?;
                for transaction_hash in &shard_touched_tx_hashes {
                    writes.note_staging_chunk(StagingChunkRow {
                        transaction_hash: transaction_hash.clone(),
                        chunk_hash: header.chunk_hash.to_string(),
                        block_height,
                        shard_id,
                        author: chunk.author.to_string(),
                        height_created: header.height_created as i64,
                        height_included: header.height_included as i64,
                        gas_used: header.gas_used.as_gas() as i64,
                        gas_limit: header.gas_limit.as_gas() as i64,
                        validator_reward: header.validator_reward.to_string(),
                        raw_data: chunk_raw_data.clone(),
                    });
                }
            }

            if !shard.state_changes.is_empty() && !shard_touched_tx_hashes.is_empty() {
                let state_changes_json =
                    Self::serialize_json(&shard.state_changes, "state changes payload")?;
                for transaction_hash in &shard_touched_tx_hashes {
                    writes.note_staging_state_changes(StagingStateChangesRow {
                        transaction_hash: transaction_hash.clone(),
                        block_height,
                        shard_id,
                        state_changes: state_changes_json.clone(),
                    });
                }
            }
        }
        Self::flush_block_writes(db, writes, &mut stats).await?;
        stats.record_phase_elapsed("staging_write", staging_write_start);

        let touched_tx_hashes: Vec<String> = touched_tx_hashes.into_iter().collect();
        stats.record_count("touched_tx_count", touched_tx_hashes.len());

        let pending_count_refresh_start = Instant::now();
        Self::refresh_pending_counts_for_txs(db, &touched_tx_hashes).await?;
        stats.record_phase_elapsed("pending_count_refresh", pending_count_refresh_start);

        let finalize_graphs_start = Instant::now();
        let finalize_candidates = Self::load_finalize_candidates(db).await?;
        stats.record_count("finalize_candidate_count", finalize_candidates.len());

        for (transaction_hash, has_hos_touch, pending_receipt_count, _first_seen_height) in
            finalize_candidates
        {
            if has_hos_touch {
                if !self.raw_enabled
                    && !Self::staging_transaction_exists(db, &transaction_hash).await?
                {
                    error!(
                        module = MODULE_NAME,
                        tx_hash = %transaction_hash,
                        pending_receipt_count,
                        block_height,
                        "HoS tx missing staged transaction row; marking incomplete and skipping promotion"
                    );
                    Self::mark_tx_state_incomplete(
                        db,
                        &transaction_hash,
                        pending_receipt_count,
                        "missing_staging_transaction_row",
                    )
                    .await?;
                    let current = stats
                        .counts()
                        .get("incomplete_tx_count")
                        .copied()
                        .unwrap_or(0);
                    stats.record_count("incomplete_tx_count", current + 1);
                    continue;
                }

                if !self.raw_enabled {
                    Self::promote_staged_tx_graph(db, &transaction_hash).await?;
                }

                Self::mark_tx_state_finished(
                    db,
                    &transaction_hash,
                    TX_STATUS_FINALIZED,
                    pending_receipt_count,
                    false,
                    None,
                )
                .await?;
                let current = stats
                    .counts()
                    .get("finalized_tx_count")
                    .copied()
                    .unwrap_or(0);
                stats.record_count("finalized_tx_count", current + 1);
            } else {
                Self::mark_tx_state_finished(
                    db,
                    &transaction_hash,
                    TX_STATUS_PRUNED_NON_HOS,
                    pending_receipt_count,
                    false,
                    None,
                )
                .await?;
                let current = stats
                    .counts()
                    .get("pruned_non_hos_tx_count")
                    .copied()
                    .unwrap_or(0);
                stats.record_count("pruned_non_hos_tx_count", current + 1);
            }

            Self::purge_staging_for_tx(db, &transaction_hash).await?;
        }
        stats.record_phase_elapsed("finalize_graphs", finalize_graphs_start);

        let staging_gc_start = Instant::now();
        self.run_staging_gc(db, block_height).await?;
        stats.record_phase_elapsed("staging_gc", staging_gc_start);

        Ok(stats)
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
            "Processing HOS governance raw block data"
        );

        let mut process_log = BlockProcessingLog::new(MODULE_NAME, height, shard_count);

        let txn = process_log
            .metrics_mut()
            .time_future("txn_begin", db.begin())
            .await?;

        let projection_stats = process_log
            .metrics_mut()
            .time_future("store_projection", self.store_hos_projection(msg, &txn))
            .await
            .context("failed to store HOS governance raw projection data")?;
        process_log
            .metrics_mut()
            .merge_with_prefix("projection.", projection_stats);

        process_log
            .metrics_mut()
            .time_future("txn_commit", txn.commit())
            .await?;

        let block_height = height as i64;
        let mview_refresh_triggered = self.should_refresh_mviews(block_height);
        process_log
            .metrics_mut()
            .record_flag("mview_refresh_triggered", mview_refresh_triggered);

        if mview_refresh_triggered {
            let mview_refresh_start = Instant::now();
            if let Err(err) = self.refresh_hos_materialized_views(db).await {
                process_log
                    .metrics_mut()
                    .record_phase_elapsed("mview_refresh", mview_refresh_start);
                process_log
                    .metrics_mut()
                    .record_flag("mview_refresh_failed", true);
                warn!(
                    module = MODULE_NAME,
                    height = block_height,
                    elapsed_ms = u64::try_from(mview_refresh_start.elapsed().as_millis())
                        .unwrap_or(u64::MAX),
                    error = %err,
                    "failed refreshing HOS materialized views"
                );
            } else {
                process_log
                    .metrics_mut()
                    .record_phase_elapsed("mview_refresh", mview_refresh_start);
                process_log
                    .metrics_mut()
                    .record_flag("mview_refresh_failed", false);
            }
        }

        process_log.log_success("Successfully processed HOS governance raw block data");

        Ok(())
    }
}

#[async_trait]
impl IndexerModule for VenearGovernanceModule {
    fn name(&self) -> &str {
        MODULE_NAME
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn stream_mode(&self) -> NearModuleStreamMode {
        self.stream_mode
    }

    async fn initialize(&self, _db: &DatabaseConnection) -> Result<()> {
        Ok(())
    }

    #[instrument(skip(self, msg, db), fields(module = "venear_governance", height = %msg.block.header.height))]
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
    use super::VenearGovernanceModule;

    #[test]
    fn hos_contract_matcher_accepts_verified_contracts_and_lockups() {
        for account_id in [
            "venear.dao",
            "vote.dao",
            "hos-root.sputnik-dao.near",
            "rewards.hos-dao.near",
            "0887fe221cafb6810909c83393e23585c439fe99.venear.dao",
            "0887FE221CAFB6810909C83393E23585C439FE99.VENEAR.DAO",
        ] {
            assert!(
                VenearGovernanceModule::is_hos_contract(account_id),
                "expected {account_id} to match HoS contract matcher"
            );
        }
    }

    #[test]
    fn hos_contract_matcher_rejects_legacy_and_unrelated_accounts() {
        for account_id in [
            "dao",
            "foo.vote.dao",
            "lockup.alice.venear.dao",
            "random.dao",
            "0887fe221cafb6810909c83393e23585c439fe9.venear.dao",
        ] {
            assert!(
                !VenearGovernanceModule::is_hos_contract(account_id),
                "expected {account_id} to be excluded from HoS contract matcher"
            );
        }
    }
}
