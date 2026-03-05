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
use tracing::{info, instrument, warn};

use crate::core::cursor;
use crate::core::receipt_tx_resolver::build_receipt_tx_hashes;
use crate::core::relevance_tracker::{
    load_relevant_receipts_for_scope, upsert_relevant_receipt, RELEVANCE_REASON_DESCENDANT,
    RELEVANCE_REASON_ROOT, RELEVANCE_SCOPE_HOS,
};
use crate::IndexerModule;

const MODULE_NAME: &str = "venear_governance";
const HOS_CONTRACT: &str = "dao";
const VENEAR_CONTRACT_ID: &str = "venear.dao";
const VOTING_CONTRACT_ID: &str = "vote.dao";
const DEFAULT_HOS_MVIEW_REFRESH_INTERVAL_BLOCKS: i64 = 100;

pub struct VenearGovernanceModule {
    enabled: bool,
    stream_mode: NearModuleStreamMode,
    mview_refresh_interval_blocks: i64,
}

impl VenearGovernanceModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        let mut enabled = false;
        let mut stream_mode = NearModuleStreamMode::Shared;
        let mut mview_refresh_interval_blocks = DEFAULT_HOS_MVIEW_REFRESH_INTERVAL_BLOCKS;

        if let common::config::AppConfig::Indexer(idx) = config {
            let idx = idx.as_ref();
            if let Some(near) = idx.near.as_ref() {
                let near_enabled = near.chain != "disabled";
                if near_enabled {
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
                    }
                }
            }
        }

        Self {
            enabled,
            stream_mode,
            mview_refresh_interval_blocks,
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

    async fn load_receipt_tx_origins(
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
             FROM near.near_venear_receipt_tx_origins
             WHERE receipt_id IN ({placeholders})"
        );

        let rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &sql,
                deduped.into_iter().map(Into::into).collect::<Vec<_>>(),
            ))
            .await
            .context("failed to load near_venear_receipt_tx_origins")?;

        let mut result = HashMap::new();
        for row in rows {
            let receipt_id: String = row
                .try_get("", "receipt_id")
                .context("failed to read near_venear_receipt_tx_origins.receipt_id")?;
            let transaction_hash: String = row
                .try_get("", "transaction_hash")
                .context("failed to read near_venear_receipt_tx_origins.transaction_hash")?;
            result.insert(receipt_id, transaction_hash);
        }

        Ok(result)
    }

    async fn upsert_receipt_tx_origin(
        db: &impl ConnectionTrait,
        receipt_id: &str,
        transaction_hash: &str,
        block_height: i64,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_venear_receipt_tx_origins (
                receipt_id,
                transaction_hash,
                first_observed_height,
                last_observed_height,
                created_at,
                updated_at
            ) VALUES ($1, $2, $3, $3, NOW(), NOW())
            ON CONFLICT (receipt_id) DO UPDATE SET
                transaction_hash = EXCLUDED.transaction_hash,
                last_observed_height = GREATEST(
                    near.near_venear_receipt_tx_origins.last_observed_height,
                    EXCLUDED.last_observed_height
                ),
                updated_at = NOW()",
            vec![
                receipt_id.into(),
                transaction_hash.into(),
                block_height.into(),
            ],
        ))
        .await
        .context("failed to upsert near_venear_receipt_tx_origins")?;

        Ok(())
    }

    fn is_hos_contract(&self, account_id: &str) -> bool {
        let account = account_id.to_ascii_lowercase();
        if account == HOS_CONTRACT || account == VENEAR_CONTRACT_ID || account == VOTING_CONTRACT_ID
        {
            return true;
        }

        account.starts_with("lockup.")
            && (account.ends_with(&format!(".{VENEAR_CONTRACT_ID}"))
                || account.ends_with(&format!(".{VOTING_CONTRACT_ID}")))
    }

    fn is_hos_related_with_signer(
        &self,
        receiver_id: &str,
        predecessor_id: &str,
        signer_account_id: Option<&str>,
    ) -> bool {
        self.is_hos_contract(receiver_id)
            || self.is_hos_contract(predecessor_id)
            || signer_account_id
                .map(|signer| self.is_hos_contract(signer))
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

    async fn refresh_hos_materialized_views(&self, db: &DatabaseConnection) {
        if let Err(err) = db
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT near.refresh_hos_materialized_views()",
                vec![],
            ))
            .await
        {
            warn!(error = %err, "failed refreshing HOS materialized views");
        }
    }

    fn should_refresh_mviews(&self, block_height: i64) -> bool {
        self.mview_refresh_interval_blocks > 0
            && block_height % self.mview_refresh_interval_blocks == 0
    }

    async fn upsert_near_block(
        &self,
        db: &impl ConnectionTrait,
        msg: &StreamerMessage,
    ) -> Result<()> {
        let header = &msg.block.header;
        let block_json = Self::serialize_json(&msg.block, "block raw_data")?;

        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_blocks (
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
                raw_data
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8,
                $9, $10, $11, $12, $13, $14, $15, $16
            )
            ON CONFLICT (height) DO NOTHING",
            vec![
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
            ],
        ))
        .await
        .context("failed to upsert near.near_blocks for HoS related block")?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_near_transaction(
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
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
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
        .context("failed to upsert near.near_transactions for HoS related transaction")?;

        Ok(())
    }

    async fn upsert_near_transaction_placeholder(
        &self,
        db: &impl ConnectionTrait,
        hash: &str,
        block_height: i64,
        shard_id: i64,
        signer_id: &str,
        receiver_id: &str,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
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
            ) VALUES ($1, $2, $3, $4, 0, 0, '', '', '[]'::jsonb, NULL, $5)
            ON CONFLICT (hash) DO NOTHING",
            vec![
                hash.into(),
                block_height.into(),
                signer_id.into(),
                receiver_id.into(),
                shard_id.into(),
            ],
        ))
        .await
        .context("failed to insert placeholder near.near_transactions row for HoS transaction")?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_receipt_edge(
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
        .context("failed to upsert near.near_receipt_edges")?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_near_receipt(
        &self,
        db: &impl ConnectionTrait,
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
            "INSERT INTO near.near_receipts (
                receipt_id,
                block_height,
                shard_id,
                predecessor_id,
                receiver_id,
                receipt,
                raw_data
            ) VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (receipt_id) DO UPDATE SET
                block_height = EXCLUDED.block_height,
                shard_id = EXCLUDED.shard_id,
                predecessor_id = EXCLUDED.predecessor_id,
                receiver_id = EXCLUDED.receiver_id,
                receipt = EXCLUDED.receipt,
                raw_data = COALESCE(near.near_receipts.raw_data, EXCLUDED.raw_data)",
            vec![
                receipt_id.into(),
                block_height.into(),
                shard_id.into(),
                predecessor_id.into(),
                receiver_id.into(),
                receipt_json.into(),
                raw_data_json.into(),
            ],
        ))
        .await
        .context("failed to upsert near.near_receipts")?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_near_execution_outcome(
        &self,
        db: &impl ConnectionTrait,
        receipt_id: &str,
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
                Some(receipt_id.to_string()).into(),
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
        .context("failed to upsert near.near_execution_outcomes")?;

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
        .context("failed to upsert near.near_receipt_actions")?;

        Ok(())
    }

    async fn store_hos_projection(
        &self,
        msg: &StreamerMessage,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let block_height = msg.block.header.height as i64;
        let block_hash = msg.block.header.hash.to_string();
        let block_timestamp = msg.block.header.timestamp as i64;
        let default_author = msg.block.author.to_string();

        let mut receipt_tx_hashes = build_receipt_tx_hashes(&msg.shards);
        let mut receipt_relations: HashMap<String, (String, String, Option<String>)> =
            HashMap::new();
        let mut has_hos_related_receipt_outcome = false;
        let mut hos_transaction_hashes: HashSet<String> = HashSet::new();
        let mut hos_transaction_fallbacks: HashMap<String, (String, String, i64)> = HashMap::new();

        for shard in &msg.shards {
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        receipt_relations
                            .entry(receipt.receipt_id.to_string())
                            .or_insert((
                                receipt.predecessor_id.to_string(),
                                receipt.receiver_id.to_string(),
                                Self::signer_account_id(&receipt.receipt).map(ToString::to_string),
                            ));
                    }
                }

                for receipt in &chunk.receipts {
                    receipt_relations
                        .entry(receipt.receipt_id.to_string())
                        .or_insert((
                            receipt.predecessor_id.to_string(),
                            receipt.receiver_id.to_string(),
                            Self::signer_account_id(&receipt.receipt).map(ToString::to_string),
                        ));
                }

                for receipt in &chunk.local_receipts {
                    receipt_relations
                        .entry(receipt.receipt_id.to_string())
                        .or_insert((
                            receipt.predecessor_id.to_string(),
                            receipt.receiver_id.to_string(),
                            Self::signer_account_id(&receipt.receipt).map(ToString::to_string),
                        ));
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                receipt_relations
                    .entry(receipt.receipt_id.to_string())
                    .or_insert((
                        receipt.predecessor_id.to_string(),
                        receipt.receiver_id.to_string(),
                        Self::signer_account_id(&receipt.receipt).map(ToString::to_string),
                    ));
            }
        }

        let mut block_receipt_ids = Vec::new();
        let mut origin_lookup_ids = Vec::new();
        for shard in &msg.shards {
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        origin_lookup_ids.push(receipt.receipt_id.to_string());
                    }
                    for child_receipt_id in &tx_with_outcome
                        .outcome
                        .execution_outcome
                        .outcome
                        .receipt_ids
                    {
                        origin_lookup_ids.push(child_receipt_id.to_string());
                    }
                }
            }
            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt_id = outcome_with_receipt.receipt.receipt_id.to_string();
                block_receipt_ids.push(receipt_id.clone());
                origin_lookup_ids.push(receipt_id);
            }
        }

        let existing_origins = Self::load_receipt_tx_origins(db, &origin_lookup_ids).await?;
        for (receipt_id, transaction_hash) in existing_origins {
            receipt_tx_hashes
                .entry(receipt_id)
                .or_insert_with(|| transaction_hash);
        }

        let mut tracked_relevant =
            load_relevant_receipts_for_scope(db, RELEVANCE_SCOPE_HOS, &block_receipt_ids).await?;
        for (receipt_id, transaction_hash) in &tracked_relevant {
            if let Some(transaction_hash) = transaction_hash {
                receipt_tx_hashes
                    .entry(receipt_id.clone())
                    .or_insert_with(|| transaction_hash.clone());
            }
        }

        for shard in &msg.shards {
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

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                let receipt_id = receipt.receipt_id.to_string();
                let predecessor_id = receipt.predecessor_id.to_string();
                let receiver_id = receipt.receiver_id.to_string();
                let is_hos_root_related = self.is_hos_related_receipt(receipt);
                let tracked_transaction_hash = tracked_relevant.get(&receipt_id).cloned().flatten();
                let is_hos_related_receipt =
                    is_hos_root_related || tracked_relevant.contains_key(&receipt_id);

                if !is_hos_related_receipt {
                    continue;
                }

                has_hos_related_receipt_outcome = true;

                let transaction_hash = receipt_tx_hashes
                    .get(&receipt_id)
                    .cloned()
                    .or(tracked_transaction_hash);
                if let Some(transaction_hash) = &transaction_hash {
                    hos_transaction_hashes.insert(transaction_hash.clone());
                    hos_transaction_fallbacks
                        .entry(transaction_hash.clone())
                        .or_insert_with(|| (predecessor_id.clone(), receiver_id.clone(), shard_id));
                }

                upsert_relevant_receipt(
                    db,
                    RELEVANCE_SCOPE_HOS,
                    &receipt_id,
                    if is_hos_root_related {
                        RELEVANCE_REASON_ROOT
                    } else {
                        RELEVANCE_REASON_DESCENDANT
                    },
                    None,
                    transaction_hash.as_deref(),
                    block_height,
                    true,
                )
                .await?;

                tracked_relevant.insert(receipt_id.clone(), transaction_hash.clone());

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

                self.upsert_near_execution_outcome(
                    db,
                    &receipt_id,
                    &outcome_view.id.to_string(),
                    transaction_hash.as_deref(),
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
                    self.upsert_receipt_edge(
                        db,
                        &receipt_id,
                        &child_receipt_id,
                        &outcome_view.id.to_string(),
                        block_height,
                        &block_hash,
                        block_timestamp,
                        transaction_hash.as_deref(),
                    )
                    .await?;

                    upsert_relevant_receipt(
                        db,
                        RELEVANCE_SCOPE_HOS,
                        &child_receipt_id,
                        RELEVANCE_REASON_DESCENDANT,
                        Some(&receipt_id),
                        transaction_hash.as_deref(),
                        block_height,
                        false,
                    )
                    .await?;

                    tracked_relevant
                        .entry(child_receipt_id.clone())
                        .and_modify(|existing| {
                            if existing.is_none() {
                                *existing = transaction_hash.clone();
                            }
                        })
                        .or_insert(transaction_hash.clone());
                    if let Some(transaction_hash) = &transaction_hash {
                        receipt_tx_hashes
                            .entry(child_receipt_id.to_string())
                            .or_insert_with(|| transaction_hash.clone());
                    }
                }

                self.upsert_near_receipt(
                    db,
                    &receipt_id,
                    block_height,
                    shard_id,
                    &predecessor_id,
                    &receiver_id,
                    Self::serialize_json(&receipt.receipt, "receipt body")?,
                    Self::serialize_json(receipt, "receipt row")?,
                )
                .await?;

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

                        self.upsert_near_receipt_action(
                            db,
                            &format!("{receipt_id}-{action_index}"),
                            block_height,
                            &receipt_id,
                            &outcome_view.id.to_string(),
                            transaction_hash.as_deref(),
                            shard_id,
                            Some(&signer_id_value),
                            Some(&signer_public_key_value),
                            Some(&gas_price_value),
                            &predecessor_id,
                            &receiver_id,
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
        }

        // Keep a global resolver cache of receipt -> transaction hash so future HoS descendants
        // can recover lineage even when roots were non-HoS at creation time.
        for (receipt_id, transaction_hash) in &receipt_tx_hashes {
            Self::upsert_receipt_tx_origin(db, receipt_id, transaction_hash, block_height).await?;
        }

        let hos_transaction_hashes_from_relevant = tracked_relevant
            .keys()
            .filter_map(|receipt_id| receipt_tx_hashes.get(receipt_id).cloned())
            .collect::<HashSet<_>>();
        let mut persisted_ho_s_transaction_hashes: HashSet<String> = HashSet::new();

        for shard in &msg.shards {
            if let Some(chunk) = &shard.chunk {
                let shard_id = Self::shard_to_i64(shard.shard_id);
                for tx_with_outcome in &chunk.transactions {
                    let tx_hash = tx_with_outcome.transaction.hash.to_string();
                    let mut root_receipt_ids = tx_with_outcome
                        .outcome
                        .execution_outcome
                        .outcome
                        .receipt_ids
                        .iter()
                        .map(|receipt_id| receipt_id.to_string())
                        .collect::<Vec<_>>();
                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        root_receipt_ids.push(receipt.receipt_id.to_string());
                    }

                    let is_hos_transaction = hos_transaction_hashes.contains(&tx_hash)
                        || hos_transaction_hashes_from_relevant.contains(&tx_hash)
                        || root_receipt_ids
                            .iter()
                            .any(|receipt_id| tracked_relevant.contains_key(receipt_id))
                        || root_receipt_ids.iter().any(|receipt_id| {
                            receipt_relations
                                .get(receipt_id)
                                .map(|(predecessor_id, receiver_id, signer_account_id)| {
                                    self.is_hos_related_with_signer(
                                        receiver_id,
                                        predecessor_id,
                                        signer_account_id.as_deref(),
                                    )
                                })
                                .unwrap_or(false)
                        });

                    if !is_hos_transaction {
                        continue;
                    }

                    self.upsert_near_transaction(
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
                    persisted_ho_s_transaction_hashes.insert(tx_hash);
                }
            }
        }

        for transaction_hash in hos_transaction_hashes
            .union(&hos_transaction_hashes_from_relevant)
            .cloned()
            .collect::<HashSet<_>>()
        {
            if persisted_ho_s_transaction_hashes.contains(&transaction_hash) {
                continue;
            }

            let (signer_id, receiver_id, shard_id) = hos_transaction_fallbacks
                .get(&transaction_hash)
                .map(|(signer_id, receiver_id, shard_id)| {
                    (signer_id.as_str(), receiver_id.as_str(), *shard_id)
                })
                .unwrap_or(("unknown", "unknown", 0));

            self.upsert_near_transaction_placeholder(
                db,
                &transaction_hash,
                block_height,
                shard_id,
                signer_id,
                receiver_id,
            )
            .await?;
        }

        if has_hos_related_receipt_outcome || !persisted_ho_s_transaction_hashes.is_empty() {
            self.upsert_near_block(db, msg).await?;
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
            "Processing HOS governance raw block data"
        );

        let txn = db.begin().await?;

        self.store_hos_projection(msg, &txn)
            .await
            .context("failed to store HOS governance raw projection data")?;

        txn.commit().await?;

        let block_height = msg.block.header.height as i64;
        if self.should_refresh_mviews(block_height) {
            self.refresh_hos_materialized_views(db).await;
        }

        let elapsed = start_time.elapsed();
        info!(
            module = MODULE_NAME,
            height = msg.block.header.height,
            shard_count = msg.shards.len(),
            elapsed_ms = elapsed.as_millis(),
            "Successfully processed HOS governance raw block data"
        );

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
