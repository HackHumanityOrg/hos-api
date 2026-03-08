use anyhow::{Context, Result};
use async_trait::async_trait;
use common::config::NearModuleStreamMode;
use near_lake_framework::near_indexer_primitives::views::ReceiptEnumView;
use near_lake_framework::near_indexer_primitives::{IndexerShard, StreamerMessage};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, HashMap};
use std::sync::Mutex;
use tracing::{error, info, instrument};

use crate::core::canonical_persistence::{self, NearReceiptWriteMode, NearTransactionWriteMode};
use crate::core::canonical_rows::{
    self, ExecutionOutcomeRowMeta, NearChunkRow, NearExecutionOutcomeRow, NearReceiptActionRow,
    NearReceiptEdgeRow, NearReceiptRow, NearStateChangesRow, NearTransactionRow,
    ReceiptActionRowMeta, ReceiptTxHashMapRow,
};
use crate::core::cursor;
use crate::core::prepared_block::PreparedBlockContext;
use crate::core::receipt_tx_canonical_resolver;
use crate::core::receipt_tx_hash_map::load_receipt_tx_hash_map;
use crate::core::stage_timing::BlockProcessingLog;
use crate::IndexerModule;

const MODULE_NAME: &str = "raw";
const DATA_RECEIPT_LINK_RETENTION_BLOCKS: i64 = 256;

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

    fn shard_to_i64(shard_id: near_lake_framework::near_indexer_primitives::types::ShardId) -> i64 {
        canonical_rows::shard_to_i64(shard_id)
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

    async fn flush_receipt_tx_hash_map_upserts(
        db: &impl ConnectionTrait,
        rows: &[ReceiptTxHashMapRow],
    ) -> Result<()> {
        canonical_persistence::flush_receipt_tx_hash_map_upserts(db, rows, MODULE_NAME).await?;
        Ok(())
    }

    async fn flush_near_chunks(db: &impl ConnectionTrait, rows: &[NearChunkRow]) -> Result<()> {
        canonical_persistence::flush_near_chunks(db, rows).await
    }

    async fn flush_near_transactions(
        db: &impl ConnectionTrait,
        rows: &[NearTransactionRow],
    ) -> Result<()> {
        canonical_persistence::flush_near_transactions(
            db,
            rows,
            NearTransactionWriteMode::InsertOnly,
        )
        .await
    }

    async fn flush_near_receipts(db: &impl ConnectionTrait, rows: &[NearReceiptRow]) -> Result<()> {
        canonical_persistence::flush_near_receipts(db, rows, NearReceiptWriteMode::InsertOnly).await
    }

    async fn flush_near_execution_outcomes(
        db: &impl ConnectionTrait,
        rows: &[NearExecutionOutcomeRow],
    ) -> Result<()> {
        canonical_persistence::flush_near_execution_outcomes(db, rows).await
    }

    async fn flush_near_receipt_actions(
        db: &impl ConnectionTrait,
        rows: &[NearReceiptActionRow],
    ) -> Result<()> {
        canonical_persistence::flush_near_receipt_actions(db, rows).await
    }

    async fn flush_near_receipt_edges(
        db: &impl ConnectionTrait,
        rows: &[NearReceiptEdgeRow],
    ) -> Result<()> {
        canonical_persistence::flush_near_receipt_edges(db, rows).await
    }

    async fn flush_near_state_changes(
        db: &impl ConnectionTrait,
        rows: &[NearStateChangesRow],
    ) -> Result<()> {
        canonical_persistence::flush_near_state_changes(db, rows).await
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

    #[allow(clippy::too_many_arguments)]
    async fn store_execution_outcomes(
        &self,
        shards: &[IndexerShard],
        candidate_receipt_ids: &[String],
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
                receipt_tx_canonical_resolver::resolve_receipt_tx_hashes_from_canonical_graph(
                    db,
                    &unresolved_receipt_ids,
                )
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
                    let execution_outcome_id = outcome_view.id.to_string();

                    outcome_rows.insert(
                        execution_outcome_id.clone(),
                        canonical_rows::execution_outcome_to_row(
                            outcome_view,
                            ExecutionOutcomeRowMeta {
                                transaction_hash: Some(&transaction_hash),
                                block_height,
                                shard_id,
                                receipt_id: receipt_id.as_deref(),
                                block_hash,
                                chunk_hash: &chunk_hash,
                                block_timestamp,
                            },
                        )?,
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
                let execution_outcome_id = outcome_view.id.to_string();

                outcome_rows.insert(
                    execution_outcome_id.clone(),
                    canonical_rows::execution_outcome_to_row(
                        outcome_view,
                        ExecutionOutcomeRowMeta {
                            transaction_hash: transaction_hash.as_deref(),
                            block_height,
                            shard_id,
                            receipt_id: Some(&receipt_id),
                            block_hash,
                            chunk_hash: &chunk_hash,
                            block_timestamp,
                        },
                    )?,
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

                for row in canonical_rows::receipt_action_rows(
                    receipt,
                    ReceiptActionRowMeta {
                        transaction_hash: transaction_hash.as_deref(),
                        block_height,
                        execution_outcome_id: &execution_outcome_id,
                        shard_id,
                        block_hash,
                        chunk_hash: &chunk_hash,
                        author: &chunk_author,
                        block_timestamp,
                    },
                )? {
                    action_rows.insert(row.id.clone(), row);
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

    async fn process_block_internal(
        &self,
        prepared: &PreparedBlockContext,
        msg: &StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()> {
        let height = prepared.block_height as u64;
        let shard_count = prepared.shard_count;
        info!(
            module = MODULE_NAME,
            height = height,
            shard_count = shard_count,
            "Processing NEAR raw block data"
        );

        let mut process_log = BlockProcessingLog::new(MODULE_NAME, height, shard_count);
        process_log
            .metrics_mut()
            .record_count("chunk_count", prepared.chunk_count);
        process_log
            .metrics_mut()
            .record_count("transaction_count", prepared.transaction_count);
        process_log
            .metrics_mut()
            .record_count("receipt_count", prepared.receipt_count);
        process_log.metrics_mut().record_count(
            "receipt_execution_outcome_count",
            prepared.receipt_execution_outcome_count,
        );
        process_log
            .metrics_mut()
            .record_count("state_change_count", prepared.state_change_count);
        process_log.metrics_mut().record_count(
            "prepared_candidate_receipt_ids_before_dedup",
            prepared.candidate_receipt_id_count_before_dedup,
        );
        process_log.metrics_mut().record_count(
            "prepared_candidate_receipt_ids_after_dedup",
            prepared.candidate_receipt_ids.len(),
        );
        process_log.metrics_mut().record_count(
            "prepared_data_receipt_id_count",
            prepared.data_receipt_ids.len(),
        );
        process_log.metrics_mut().record_count(
            "prepared_receipt_tx_seed_count",
            prepared.receipt_tx_hashes.len(),
        );

        let block_height = prepared.block_height;
        let block_hash = prepared.block_hash.as_str();
        let block_timestamp = prepared.block_timestamp;
        let block_author = prepared.block_author.as_str();
        let receipt_tx_hashes = &prepared.receipt_tx_hashes;

        let txn = process_log
            .metrics_mut()
            .time_future("txn_begin", db.begin())
            .await?;

        process_log
            .metrics_mut()
            .time_future(
                "store_block",
                canonical_persistence::flush_near_blocks(
                    &txn,
                    std::slice::from_ref(&prepared.block_row),
                ),
            )
            .await
            .context("Failed to store block data")?;

        process_log
            .metrics_mut()
            .time_future(
                "store_chunks",
                Self::flush_near_chunks(&txn, &prepared.chunk_rows),
            )
            .await
            .context("Failed to store chunk data")?;

        process_log
            .metrics_mut()
            .time_future(
                "store_transactions",
                Self::flush_near_transactions(&txn, &prepared.transaction_rows),
            )
            .await
            .context("Failed to store transaction data")?;

        process_log
            .metrics_mut()
            .time_future(
                "store_receipts",
                Self::flush_near_receipts(&txn, &prepared.receipt_rows),
            )
            .await
            .context("Failed to store receipt data")?;

        process_log
            .metrics_mut()
            .time_future(
                "store_execution_outcomes",
                self.store_execution_outcomes(
                    &msg.shards,
                    &prepared.candidate_receipt_ids,
                    block_height,
                    block_hash,
                    block_timestamp,
                    block_author,
                    receipt_tx_hashes,
                    &txn,
                ),
            )
            .await
            .context("Failed to store execution outcome data")?;

        process_log
            .metrics_mut()
            .time_future(
                "store_state_changes",
                Self::flush_near_state_changes(&txn, &prepared.state_change_rows),
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

    #[instrument(skip(self, prepared, msg, db), fields(module = "raw", height = prepared.block_height))]
    async fn process_prepared_block(
        &self,
        prepared: &PreparedBlockContext,
        msg: &StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()> {
        self.process_block_internal(prepared, msg, db).await
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
