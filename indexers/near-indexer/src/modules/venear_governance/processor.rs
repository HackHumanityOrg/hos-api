use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use common::config::NearModuleStreamMode;
use near_lake_framework::near_indexer_primitives::views::{
    ActionView, ExecutionStatusView, ReceiptEnumView,
};
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::{json, Value as JsonValue};
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tracing::{error, info, instrument, warn};

use crate::core::cursor;
use crate::core::receipt_tx_hash_map::{load_receipt_tx_hash_map, upsert_receipt_tx_hash};
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

    async fn upsert_tx_state(
        db: &impl ConnectionTrait,
        transaction_hash: &str,
        block_height: i64,
        has_hos_touch: bool,
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
                created_at,
                updated_at
            ) VALUES ($1, $1, $2, 0, $3, $4, $4, NOW(), NOW())
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
            vec![
                transaction_hash.into(),
                TX_STATUS_PENDING.into(),
                has_hos_touch.into(),
                block_height.into(),
            ],
        ))
        .await
        .context("failed to upsert near_hos_tx_state")?;

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

    async fn upsert_staging_block_for_tx(
        &self,
        db: &impl ConnectionTrait,
        transaction_hash: &str,
        msg: &StreamerMessage,
        block_height: i64,
    ) -> Result<()> {
        let header = &msg.block.header;
        let block_json = Self::serialize_json(&msg.block, "staging block raw_data")?;

        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
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
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8,
                $9, $10, $11, $12, $13, $14, $15,
                $16, $17, $18, $18
            )
            ON CONFLICT (transaction_hash, height) DO UPDATE SET
                last_seen_height = GREATEST(
                    near.near_hos_staging_blocks.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            vec![
                transaction_hash.into(),
                (header.height as i64).into(),
                header.hash.to_string().into(),
                header.prev_hash.to_string().into(),
                header.epoch_id.to_string().into(),
                header.next_epoch_id.to_string().into(),
                msg.block.author.to_string().into(),
                (header.timestamp as i64).into(),
                (header.timestamp_nanosec as i64).into(),
                header.gas_price.to_string().into(),
                header.total_supply.to_string().into(),
                header.validator_reward.to_string().into(),
                (header.latest_protocol_version as i32).into(),
                (header.chunks_included as i64).into(),
                header.block_ordinal.map(|value| value as i64).into(),
                header.prev_height.map(|value| value as i64).into(),
                Some(block_json).into(),
                block_height.into(),
            ],
        ))
        .await
        .context("failed to upsert near_hos_staging_blocks")?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_staging_chunk_for_tx(
        &self,
        db: &impl ConnectionTrait,
        transaction_hash: &str,
        chunk_hash: &str,
        block_height: i64,
        shard_id: i64,
        author: &str,
        height_created: i64,
        height_included: i64,
        gas_used: i64,
        gas_limit: i64,
        validator_reward: &str,
        raw_data_json: JsonValue,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
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
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9, $10, $11, $3, $3
            )
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
            vec![
                transaction_hash.into(),
                chunk_hash.into(),
                block_height.into(),
                shard_id.into(),
                author.into(),
                height_created.into(),
                height_included.into(),
                gas_used.into(),
                gas_limit.into(),
                validator_reward.into(),
                raw_data_json.into(),
            ],
        ))
        .await
        .context("failed to upsert near_hos_staging_chunks")?;

        Ok(())
    }

    async fn upsert_staging_state_changes_for_tx(
        &self,
        db: &impl ConnectionTrait,
        transaction_hash: &str,
        block_height: i64,
        shard_id: i64,
        state_changes_json: JsonValue,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_hos_staging_state_changes (
                transaction_hash,
                block_height,
                shard_id,
                state_changes,
                first_seen_height,
                last_seen_height
            ) VALUES ($1, $2, $3, $4, $2, $2)
            ON CONFLICT (transaction_hash, block_height, shard_id) DO UPDATE SET
                state_changes = EXCLUDED.state_changes,
                last_seen_height = GREATEST(
                    near.near_hos_staging_state_changes.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            vec![
                transaction_hash.into(),
                block_height.into(),
                shard_id.into(),
                state_changes_json.into(),
            ],
        ))
        .await
        .context("failed to upsert near_hos_staging_state_changes")?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_staging_transaction(
        &self,
        db: &impl ConnectionTrait,
        hash: &str,
        block_height: i64,
        shard_id: i64,
        signer_id: &str,
        receiver_id: &str,
        nonce: i64,
        priority_fee: i64,
        public_key: &str,
        signature: &str,
        actions_json: JsonValue,
        raw_data_json: JsonValue,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
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
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                $8, $9, $10, $11, $2, $2
            )
            ON CONFLICT (hash) DO UPDATE SET
                block_height = EXCLUDED.block_height,
                signer_id = EXCLUDED.signer_id,
                receiver_id = EXCLUDED.receiver_id,
                nonce = EXCLUDED.nonce,
                priority_fee = EXCLUDED.priority_fee,
                public_key = EXCLUDED.public_key,
                signature = EXCLUDED.signature,
                actions = EXCLUDED.actions,
                raw_data = COALESCE(EXCLUDED.raw_data, near.near_hos_staging_transactions.raw_data),
                shard_id = EXCLUDED.shard_id,
                last_seen_height = GREATEST(
                    near.near_hos_staging_transactions.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            vec![
                hash.into(),
                block_height.into(),
                signer_id.into(),
                receiver_id.into(),
                nonce.into(),
                priority_fee.into(),
                public_key.into(),
                signature.into(),
                actions_json.into(),
                raw_data_json.into(),
                shard_id.into(),
            ],
        ))
        .await
        .context("failed to upsert near_hos_staging_transactions")?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_staging_receipt(
        &self,
        db: &impl ConnectionTrait,
        transaction_hash: &str,
        receipt_id: &str,
        block_height: i64,
        shard_id: i64,
        predecessor_id: &str,
        receiver_id: &str,
        receipt_json: JsonValue,
        raw_data_json: JsonValue,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
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
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $3, $3)
            ON CONFLICT (receipt_id) DO UPDATE SET
                transaction_hash = EXCLUDED.transaction_hash,
                block_height = EXCLUDED.block_height,
                shard_id = EXCLUDED.shard_id,
                predecessor_id = EXCLUDED.predecessor_id,
                receiver_id = EXCLUDED.receiver_id,
                receipt = EXCLUDED.receipt,
                raw_data = COALESCE(near.near_hos_staging_receipts.raw_data, EXCLUDED.raw_data),
                last_seen_height = GREATEST(
                    near.near_hos_staging_receipts.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            vec![
                receipt_id.into(),
                transaction_hash.into(),
                block_height.into(),
                shard_id.into(),
                predecessor_id.into(),
                receiver_id.into(),
                receipt_json.into(),
                raw_data_json.into(),
            ],
        ))
        .await
        .context("failed to upsert near_hos_staging_receipts")?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_staging_execution_outcome(
        &self,
        db: &impl ConnectionTrait,
        transaction_hash: &str,
        receipt_id: Option<&str>,
        execution_outcome_id: &str,
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
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15,
                $16, $17, $18, $19, $3,
                $3
            )
            ON CONFLICT (id) DO UPDATE SET
                transaction_hash = EXCLUDED.transaction_hash,
                block_height = EXCLUDED.block_height,
                shard_id = EXCLUDED.shard_id,
                receipt_id = COALESCE(near.near_hos_staging_execution_outcomes.receipt_id, EXCLUDED.receipt_id),
                outcome = EXCLUDED.outcome,
                raw_data = COALESCE(near.near_hos_staging_execution_outcomes.raw_data, EXCLUDED.raw_data),
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
            vec![
                execution_outcome_id.into(),
                transaction_hash.into(),
                block_height.into(),
                shard_id.into(),
                receipt_id.map(ToString::to_string).into(),
                outcome_json.into(),
                raw_data_json.into(),
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
        .context("failed to upsert near_hos_staging_execution_outcomes")?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_staging_receipt_action(
        &self,
        db: &impl ConnectionTrait,
        id: &str,
        transaction_hash: &str,
        block_height: i64,
        receipt_id: &str,
        execution_outcome_id: &str,
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
            ) VALUES (
                $1, $2, $3, $4, $5,
                $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15,
                $16, $17, $18, $19, $20,
                $21, $22, $23, $3, $3
            )
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
            vec![
                id.into(),
                transaction_hash.into(),
                block_height.into(),
                receipt_id.into(),
                execution_outcome_id.into(),
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
        .context("failed to upsert near_hos_staging_receipt_actions")?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_staging_receipt_edge(
        &self,
        db: &impl ConnectionTrait,
        transaction_hash: &str,
        parent_receipt_id: &str,
        child_receipt_id: &str,
        parent_execution_outcome_id: &str,
        block_height: i64,
        block_hash: &str,
        block_timestamp: i64,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
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
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $5, $5)
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
            vec![
                parent_receipt_id.into(),
                child_receipt_id.into(),
                transaction_hash.into(),
                parent_execution_outcome_id.into(),
                block_height.into(),
                block_hash.into(),
                block_timestamp.into(),
            ],
        ))
        .await
        .context("failed to upsert near_hos_staging_receipt_edges")?;

        Ok(())
    }

    async fn upsert_staging_receipt_status(
        &self,
        db: &impl ConnectionTrait,
        transaction_hash: &str,
        receipt_id: &str,
        executed: bool,
        block_height: i64,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_hos_staging_receipt_status (
                receipt_id,
                transaction_hash,
                executed,
                first_seen_height,
                last_seen_height
            ) VALUES ($1, $2, $3, $4, $4)
            ON CONFLICT (receipt_id) DO UPDATE SET
                transaction_hash = EXCLUDED.transaction_hash,
                executed = near.near_hos_staging_receipt_status.executed OR EXCLUDED.executed,
                last_seen_height = GREATEST(
                    near.near_hos_staging_receipt_status.last_seen_height,
                    EXCLUDED.last_seen_height
                )",
            vec![
                receipt_id.into(),
                transaction_hash.into(),
                executed.into(),
                block_height.into(),
            ],
        ))
        .await
        .context("failed to upsert near_hos_staging_receipt_status")?;

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
                    Self::upsert_tx_state(db, &tx_hash, block_height, tx_root_related).await?;
                    self.upsert_staging_block_for_tx(db, &tx_hash, msg, block_height)
                        .await?;
                    self.upsert_staging_transaction(
                        db,
                        &tx_hash,
                        block_height,
                        shard_id,
                        tx_with_outcome.transaction.signer_id.as_ref(),
                        tx_with_outcome.transaction.receiver_id.as_ref(),
                        tx_with_outcome.transaction.nonce as i64,
                        tx_with_outcome.transaction.priority_fee as i64,
                        &tx_with_outcome.transaction.public_key.to_string(),
                        &tx_with_outcome.transaction.signature.to_string(),
                        Self::serialize_json(
                            &tx_with_outcome.transaction.actions,
                            "HoS transaction actions",
                        )?,
                        Self::serialize_json(
                            &tx_with_outcome.transaction,
                            "HoS transaction raw_data",
                        )?,
                    )
                    .await?;

                    let tx_outcome_receipt_id = tx_with_outcome
                        .outcome
                        .receipt
                        .as_ref()
                        .map(|receipt| receipt.receipt_id.to_string());
                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        let receipt_id = receipt.receipt_id.to_string();
                        receipt_tx_hashes.insert(receipt_id.clone(), tx_hash.clone());
                        Self::persist_receipt_tx_mapping(db, &receipt_id, &tx_hash, block_height)
                            .await?;

                        let is_hos_root_related = self.is_hos_related_receipt(receipt);
                        if is_hos_root_related {
                            Self::upsert_tx_state(db, &tx_hash, block_height, true).await?;
                        }

                        self.upsert_staging_receipt(
                            db,
                            &tx_hash,
                            &receipt_id,
                            block_height,
                            shard_id,
                            receipt.predecessor_id.as_ref(),
                            receipt.receiver_id.as_ref(),
                            Self::serialize_json(&receipt.receipt, "receipt body")?,
                            Self::serialize_json(receipt, "receipt row")?,
                        )
                        .await?;
                        self.upsert_staging_receipt_status(
                            db,
                            &tx_hash,
                            &receipt_id,
                            true,
                            block_height,
                        )
                        .await?;
                    }

                    let tx_outcome_view = &tx_with_outcome.outcome.execution_outcome;
                    let tx_outcome = &tx_outcome_view.outcome;
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
                    self.upsert_staging_execution_outcome(
                        db,
                        &tx_hash,
                        tx_outcome_receipt_id.as_deref(),
                        &tx_outcome_view.id.to_string(),
                        block_height,
                        &block_hash,
                        &chunk_hash,
                        shard_id,
                        tx_outcome.gas_burnt.as_gas() as i64,
                        tx_outcome.gas_burnt.as_gas() as f64,
                        tx_outcome.tokens_burnt.as_yoctonear() as f64 / 1e24,
                        tx_outcome.executor_id.as_ref(),
                        tx_status,
                        tx_outcome_receipt_ids,
                        tx_logs,
                        tx_results_json,
                        Self::serialize_json(&tx_outcome_view.outcome, "tx execution outcome")?,
                        Self::serialize_json(tx_outcome_view, "tx execution outcome row")?,
                        block_timestamp,
                    )
                    .await?;

                    if let Some(parent_receipt_id) = tx_outcome_receipt_id.as_deref() {
                        for child_receipt_id in &tx_outcome.receipt_ids {
                            let child_receipt_id = child_receipt_id.to_string();
                            receipt_tx_hashes.insert(child_receipt_id.clone(), tx_hash.clone());
                            Self::persist_receipt_tx_mapping(
                                db,
                                &child_receipt_id,
                                &tx_hash,
                                block_height,
                            )
                            .await?;
                            self.upsert_staging_receipt_edge(
                                db,
                                &tx_hash,
                                parent_receipt_id,
                                &child_receipt_id,
                                &tx_outcome_view.id.to_string(),
                                block_height,
                                &block_hash,
                                block_timestamp,
                            )
                            .await?;
                            self.upsert_staging_receipt_status(
                                db,
                                &tx_hash,
                                &child_receipt_id,
                                false,
                                block_height,
                            )
                            .await?;
                        }
                    } else {
                        for child_receipt_id in &tx_outcome.receipt_ids {
                            let child_receipt_id = child_receipt_id.to_string();
                            receipt_tx_hashes.insert(child_receipt_id.clone(), tx_hash.clone());
                            Self::persist_receipt_tx_mapping(
                                db,
                                &child_receipt_id,
                                &tx_hash,
                                block_height,
                            )
                            .await?;
                            self.upsert_staging_receipt_status(
                                db,
                                &tx_hash,
                                &child_receipt_id,
                                false,
                                block_height,
                            )
                            .await?;
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
                    Self::upsert_tx_state(db, &transaction_hash, block_height, is_hos_root_related)
                        .await?;
                    Self::persist_receipt_tx_mapping(
                        db,
                        &receipt_id,
                        &transaction_hash,
                        block_height,
                    )
                    .await?;
                    self.upsert_staging_block_for_tx(db, &transaction_hash, msg, block_height)
                        .await?;
                    self.upsert_staging_receipt(
                        db,
                        &transaction_hash,
                        &receipt_id,
                        block_height,
                        shard_id,
                        receipt.predecessor_id.as_ref(),
                        receipt.receiver_id.as_ref(),
                        Self::serialize_json(&receipt.receipt, "observed receipt body")?,
                        Self::serialize_json(receipt, "observed receipt row")?,
                    )
                    .await?;
                    self.upsert_staging_receipt_status(
                        db,
                        &transaction_hash,
                        &receipt_id,
                        false,
                        block_height,
                    )
                    .await?;
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
                Self::upsert_tx_state(db, &transaction_hash, block_height, is_hos_root_related)
                    .await?;
                Self::persist_receipt_tx_mapping(db, &receipt_id, &transaction_hash, block_height)
                    .await?;
                self.upsert_staging_block_for_tx(db, &transaction_hash, msg, block_height)
                    .await?;

                self.upsert_staging_receipt(
                    db,
                    &transaction_hash,
                    &receipt_id,
                    block_height,
                    shard_id,
                    receipt.predecessor_id.as_ref(),
                    receipt.receiver_id.as_ref(),
                    Self::serialize_json(&receipt.receipt, "receipt body")?,
                    Self::serialize_json(receipt, "receipt row")?,
                )
                .await?;
                self.upsert_staging_receipt_status(
                    db,
                    &transaction_hash,
                    &receipt_id,
                    true,
                    block_height,
                )
                .await?;

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

                self.upsert_staging_execution_outcome(
                    db,
                    &transaction_hash,
                    Some(&receipt_id),
                    &outcome_view.id.to_string(),
                    block_height,
                    &block_hash,
                    &chunk_hash,
                    shard_id,
                    outcome.gas_burnt.as_gas() as i64,
                    outcome.gas_burnt.as_gas() as f64,
                    outcome.tokens_burnt.as_yoctonear() as f64 / 1e24,
                    &executor_account_id,
                    status,
                    outcome_receipt_ids.clone(),
                    logs,
                    results_json,
                    Self::serialize_json(&outcome_view.outcome, "execution outcome")?,
                    Self::serialize_json(outcome_view, "execution outcome row")?,
                    block_timestamp,
                )
                .await?;

                for child_receipt_id in &outcome.receipt_ids {
                    let child_receipt_id = child_receipt_id.to_string();
                    receipt_tx_hashes.insert(child_receipt_id.clone(), transaction_hash.clone());
                    Self::persist_receipt_tx_mapping(
                        db,
                        &child_receipt_id,
                        &transaction_hash,
                        block_height,
                    )
                    .await?;
                    self.upsert_staging_receipt_edge(
                        db,
                        &transaction_hash,
                        &receipt_id,
                        &child_receipt_id,
                        &outcome_view.id.to_string(),
                        block_height,
                        &block_hash,
                        block_timestamp,
                    )
                    .await?;
                    self.upsert_staging_receipt_status(
                        db,
                        &transaction_hash,
                        &child_receipt_id,
                        false,
                        block_height,
                    )
                    .await?;
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

                        self.upsert_staging_receipt_action(
                            db,
                            &format!("{receipt_id}-{action_index}"),
                            &transaction_hash,
                            block_height,
                            &receipt_id,
                            &outcome_view.id.to_string(),
                            shard_id,
                            Some(&signer_id_value),
                            Some(&signer_public_key_value),
                            Some(&gas_price_value),
                            receipt.predecessor_id.as_ref(),
                            receipt.receiver_id.as_ref(),
                            &block_hash,
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

            if let Some(chunk) = &shard.chunk {
                let header = &chunk.header;
                let chunk_raw_data = Self::serialize_json(chunk, "chunk raw_data")?;
                for transaction_hash in &shard_touched_tx_hashes {
                    self.upsert_staging_chunk_for_tx(
                        db,
                        transaction_hash,
                        &header.chunk_hash.to_string(),
                        block_height,
                        shard_id,
                        chunk.author.as_ref(),
                        header.height_created as i64,
                        header.height_included as i64,
                        header.gas_used.as_gas() as i64,
                        header.gas_limit.as_gas() as i64,
                        &header.validator_reward.to_string(),
                        chunk_raw_data.clone(),
                    )
                    .await?;
                }
            }

            if !shard.state_changes.is_empty() && !shard_touched_tx_hashes.is_empty() {
                let state_changes_json =
                    Self::serialize_json(&shard.state_changes, "state changes payload")?;
                for transaction_hash in &shard_touched_tx_hashes {
                    self.upsert_staging_state_changes_for_tx(
                        db,
                        transaction_hash,
                        block_height,
                        shard_id,
                        state_changes_json.clone(),
                    )
                    .await?;
                }
            }
        }
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
