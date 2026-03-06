use anyhow::{Context, Result};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use common::config::NearModuleStreamMode;
use near_lake_framework::near_indexer_primitives::views::{
    ActionView, ExecutionStatusView, ReceiptEnumView, ReceiptView,
};
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, TransactionTrait,
    Value,
};
use serde_json::{json, Value as JsonValue};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
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
const DEFAULT_FINALIZATION_INTERVAL_BLOCKS: i64 = 100;
const DEFAULT_TX_MAP_RETENTION_BLOCKS: i64 = 1_024;
const DEFAULT_YIELD_TIMEOUT_BLOCKS: i64 = 200;
const DATA_RECEIPT_LINK_RETENTION_BLOCKS: i64 = 256;
const STALLED_PENDING_GRAPH_WARNING_BLOCKS: i64 = 43_200;
const RECEIPT_TX_MAP_CLEANUP_BATCH_SIZE: usize = 5_000;
const RECEIPT_TX_MAP_CLEANUP_MAX_BATCHES: usize = 50;
const MAX_BULK_INSERT_PARAMS: usize = 65_535;
const MAX_BULK_INSERT_ROWS: usize = 250;

#[derive(Clone)]
struct ReceiptTxHashMapRow {
    receipt_id: String,
    transaction_hash: String,
    block_height: i64,
}

#[derive(Clone)]
struct BlockSource {
    author: String,
    block: Arc<near_lake_framework::near_indexer_primitives::views::BlockView>,
}

#[derive(Clone)]
struct ChunkSource {
    block_height: i64,
    shard_id: i64,
    chunk: Arc<near_lake_framework::near_indexer_primitives::IndexerChunkView>,
}

#[derive(Clone)]
struct StateChangesSource {
    block_height: i64,
    shard_id: i64,
    state_changes: Arc<near_lake_framework::near_indexer_primitives::views::StateChangesView>,
}

#[derive(Clone)]
struct TransactionSource {
    block_height: i64,
    shard_id: i64,
    transaction: Arc<near_lake_framework::near_indexer_primitives::views::SignedTransactionView>,
}

#[derive(Clone)]
struct ReceiptSource {
    block_height: i64,
    shard_id: i64,
    receipt: Arc<ReceiptView>,
}

#[derive(Clone)]
struct ExecutionOutcomeSource {
    id: String,
    transaction_hash: String,
    block_height: i64,
    shard_id: i64,
    receipt_id: Option<String>,
    execution_outcome:
        Arc<near_lake_framework::near_indexer_primitives::views::ExecutionOutcomeWithIdView>,
    block_hash: String,
    chunk_hash: String,
    author: String,
    block_timestamp: i64,
    receipt: Option<Arc<ReceiptView>>,
}

#[derive(Clone)]
struct NearBlockRow {
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
struct NearReceiptActionRow {
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
struct NearReceiptEdgeRow {
    parent_receipt_id: String,
    child_receipt_id: String,
    transaction_hash: String,
    parent_execution_outcome_id: String,
    block_height: i64,
    block_hash: String,
    block_timestamp: i64,
}

#[derive(Clone)]
struct DataReceiptParentLink {
    transaction_hash: String,
    parent_receipt_id: String,
    parent_execution_outcome_id: String,
    block_height: i64,
    block_hash: String,
    block_timestamp: i64,
}

#[derive(Default, Clone)]
struct HosGraphBundle {
    blocks: HashMap<i64, NearBlockRow>,
    chunks: HashMap<String, NearChunkRow>,
    state_changes: HashMap<(i64, i64), NearStateChangesRow>,
    transactions: HashMap<String, NearTransactionRow>,
    receipts: HashMap<String, NearReceiptRow>,
    execution_outcomes: HashMap<String, NearExecutionOutcomeRow>,
    receipt_actions: HashMap<String, NearReceiptActionRow>,
    receipt_edges: HashMap<(String, String), NearReceiptEdgeRow>,
}

impl HosGraphBundle {
    fn merge(&mut self, other: HosGraphBundle) {
        for (key, row) in other.blocks {
            self.blocks.entry(key).or_insert(row);
        }
        for (key, row) in other.chunks {
            self.chunks.entry(key).or_insert(row);
        }
        for (key, row) in other.state_changes {
            self.state_changes.entry(key).or_insert(row);
        }
        for (key, row) in other.transactions {
            self.transactions.entry(key).or_insert(row);
        }
        for (key, row) in other.receipts {
            self.receipts.entry(key).or_insert(row);
        }
        for (key, row) in other.execution_outcomes {
            self.execution_outcomes
                .entry(key)
                .and_modify(|existing| {
                    if existing.receipt_id.is_none() {
                        existing.receipt_id = row.receipt_id.clone();
                    }
                })
                .or_insert(row);
        }
        for (key, row) in other.receipt_actions {
            self.receipt_actions.entry(key).or_insert(row);
        }
        for (key, row) in other.receipt_edges {
            self.receipt_edges.entry(key).or_insert(row);
        }
    }

    fn has_root_transaction(&self, transaction_hash: &str) -> bool {
        self.transactions.contains_key(transaction_hash)
    }

    fn row_count(&self) -> usize {
        self.blocks.len()
            + self.chunks.len()
            + self.state_changes.len()
            + self.transactions.len()
            + self.receipts.len()
            + self.execution_outcomes.len()
            + self.receipt_actions.len()
            + self.receipt_edges.len()
    }
}

#[derive(Default, Clone)]
struct TxSliceRefs {
    block_heights: HashSet<i64>,
    chunk_hashes: HashSet<String>,
    state_change_keys: HashSet<(i64, i64)>,
    transaction_hashes: HashSet<String>,
    receipt_ids: HashSet<String>,
    execution_outcome_ids: HashSet<String>,
    receipt_edge_keys: HashSet<(String, String)>,
}

impl TxSliceRefs {
    fn merge(&mut self, other: TxSliceRefs) {
        self.block_heights.extend(other.block_heights);
        self.chunk_hashes.extend(other.chunk_hashes);
        self.state_change_keys.extend(other.state_change_keys);
        self.transaction_hashes.extend(other.transaction_hashes);
        self.receipt_ids.extend(other.receipt_ids);
        self.execution_outcome_ids
            .extend(other.execution_outcome_ids);
        self.receipt_edge_keys.extend(other.receipt_edge_keys);
    }

    fn is_empty(&self) -> bool {
        self.block_heights.is_empty()
            && self.chunk_hashes.is_empty()
            && self.state_change_keys.is_empty()
            && self.transaction_hashes.is_empty()
            && self.receipt_ids.is_empty()
            && self.execution_outcome_ids.is_empty()
            && self.receipt_edge_keys.is_empty()
    }
}

#[derive(Default)]
struct GraphBlockUpdate {
    has_hos_touch: bool,
    root_transaction_seen: bool,
    pending_receipts_add: HashSet<String>,
    pending_receipts_remove: HashSet<String>,
    yielded_receipts: HashMap<String, i64>,
    resumed_yield_receipt_count: usize,
    refs: TxSliceRefs,
}

#[derive(Default)]
struct HosBlockUpdates {
    receipt_tx_hash_map: HashMap<String, ReceiptTxHashMapRow>,
    data_receipt_links: HashMap<String, DataReceiptParentLink>,
    graph_updates: HashMap<String, GraphBlockUpdate>,
    shared_bundle: SourceBlockBundle,
}

#[derive(Default, Clone)]
struct SourceBlockBundle {
    block: Option<BlockSource>,
    chunks: HashMap<String, ChunkSource>,
    state_changes: HashMap<(i64, i64), StateChangesSource>,
    transactions: HashMap<String, TransactionSource>,
    receipts: HashMap<String, ReceiptSource>,
    execution_outcomes: HashMap<String, ExecutionOutcomeSource>,
    receipt_edges: HashMap<(String, String), NearReceiptEdgeRow>,
}

impl SourceBlockBundle {
    fn row_count(&self) -> usize {
        usize::from(self.block.is_some())
            + self.chunks.len()
            + self.state_changes.len()
            + self.transactions.len()
            + self.receipts.len()
            + self.execution_outcomes.len()
            + self.receipt_edges.len()
    }
}

impl HosBlockUpdates {
    fn graph_update_mut(&mut self, transaction_hash: &str) -> &mut GraphBlockUpdate {
        self.graph_updates
            .entry(transaction_hash.to_string())
            .or_default()
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

    fn note_output_data_receivers(
        &mut self,
        receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
        transaction_hash: &str,
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
            self.data_receipt_links
                .entry(data_receiver.data_id.to_string())
                .or_insert_with(|| DataReceiptParentLink {
                    transaction_hash: transaction_hash.to_string(),
                    parent_receipt_id: receipt.receipt_id.to_string(),
                    parent_execution_outcome_id: parent_execution_outcome_id.to_string(),
                    block_height,
                    block_hash: block_hash.to_string(),
                    block_timestamp,
                });
        }
    }

    fn note_tx_metadata(
        &mut self,
        transaction_hash: &str,
        has_hos_touch: bool,
        root_transaction_seen: bool,
    ) {
        let update = self.graph_update_mut(transaction_hash);
        update.has_hos_touch |= has_hos_touch;
        update.root_transaction_seen |= root_transaction_seen;
    }

    fn note_pending_receipt(&mut self, transaction_hash: &str, receipt_id: &str) {
        self.graph_update_mut(transaction_hash)
            .pending_receipts_add
            .insert(receipt_id.to_string());
    }

    fn note_executed_receipt(&mut self, transaction_hash: &str, receipt_id: &str) {
        self.graph_update_mut(transaction_hash)
            .pending_receipts_remove
            .insert(receipt_id.to_string());
    }

    fn note_yielded_receipt(
        &mut self,
        transaction_hash: &str,
        receipt_id: &str,
        timeout_height: i64,
    ) {
        self.graph_update_mut(transaction_hash)
            .yielded_receipts
            .insert(receipt_id.to_string(), timeout_height);
    }

    fn note_resumed_yield_receipt(&mut self, transaction_hash: &str) {
        self.graph_update_mut(transaction_hash)
            .resumed_yield_receipt_count += 1;
    }

    fn set_block_source(&mut self, row: BlockSource) {
        if self.shared_bundle.block.is_none() {
            self.shared_bundle.block = Some(row);
        }
    }

    fn note_block_ref(&mut self, transaction_hash: &str, block_height: i64) {
        self.graph_update_mut(transaction_hash)
            .refs
            .block_heights
            .insert(block_height);
    }

    fn note_chunk(&mut self, transaction_hash: &str, row: ChunkSource) {
        let key = row.chunk.header.chunk_hash.to_string();
        self.shared_bundle.chunks.entry(key.clone()).or_insert(row);
        self.graph_update_mut(transaction_hash)
            .refs
            .chunk_hashes
            .insert(key);
    }

    fn note_state_changes(&mut self, transaction_hash: &str, row: StateChangesSource) {
        let key = (row.block_height, row.shard_id);
        self.shared_bundle.state_changes.entry(key).or_insert(row);
        self.graph_update_mut(transaction_hash)
            .refs
            .state_change_keys
            .insert(key);
    }

    fn note_transaction(&mut self, transaction_hash: &str, row: TransactionSource) {
        let key = row.transaction.hash.to_string();
        self.shared_bundle
            .transactions
            .entry(key.clone())
            .or_insert(row);
        self.graph_update_mut(transaction_hash)
            .refs
            .transaction_hashes
            .insert(key);
    }

    fn note_receipt(&mut self, transaction_hash: &str, row: ReceiptSource) {
        let key = row.receipt.receipt_id.to_string();
        self.shared_bundle
            .receipts
            .entry(key.clone())
            .or_insert(row);
        self.graph_update_mut(transaction_hash)
            .refs
            .receipt_ids
            .insert(key);
    }

    fn note_execution_outcome(&mut self, transaction_hash: &str, row: ExecutionOutcomeSource) {
        let key = row.id.clone();
        self.shared_bundle
            .execution_outcomes
            .entry(key.clone())
            .or_insert(row);
        self.graph_update_mut(transaction_hash)
            .refs
            .execution_outcome_ids
            .insert(key);
    }

    fn note_receipt_edge(&mut self, transaction_hash: &str, row: NearReceiptEdgeRow) {
        let key = (row.parent_receipt_id.clone(), row.child_receipt_id.clone());
        self.shared_bundle
            .receipt_edges
            .entry(key.clone())
            .or_insert(row);
        self.graph_update_mut(transaction_hash)
            .refs
            .receipt_edge_keys
            .insert(key);
    }

    fn row_count(&self) -> usize {
        self.receipt_tx_hash_map.len() + self.shared_bundle.row_count()
    }
}

#[derive(Default)]
struct GraphState {
    first_seen_height: i64,
    last_seen_height: i64,
    has_hos_touch: bool,
    root_transaction_seen: bool,
    pending_receipts: HashSet<String>,
    open_yield_receipts: HashMap<String, i64>,
    recent_refs: BTreeMap<i64, TxSliceRefs>,
    bundle: Option<HosGraphBundle>,
    incomplete_reason: Option<String>,
}

impl GraphState {
    fn expire_timed_out_yields(&mut self, block_height: i64) {
        self.open_yield_receipts
            .retain(|_, timeout_height| *timeout_height > block_height);
    }

    fn resolve_yield_receipts(&mut self, resume_count: usize) {
        if resume_count == 0 || self.open_yield_receipts.is_empty() {
            return;
        }

        let mut yielded_receipts = self
            .open_yield_receipts
            .iter()
            .map(|(receipt_id, timeout_height)| (receipt_id.clone(), *timeout_height))
            .collect::<Vec<_>>();
        yielded_receipts.sort_by_key(|(_, timeout_height)| *timeout_height);

        for (receipt_id, _) in yielded_receipts.into_iter().take(resume_count) {
            self.open_yield_receipts.remove(&receipt_id);
        }
    }

    fn is_closed(&self) -> bool {
        self.pending_receipts.is_empty() && self.open_yield_receipts.is_empty()
    }

    fn open_graph_age_blocks(&self, current_height: i64) -> i64 {
        current_height.saturating_sub(self.first_seen_height)
    }
}

#[derive(Default)]
struct HosRuntimeState {
    graphs: HashMap<String, GraphState>,
    recent_blocks: BTreeMap<i64, RecentBlockCache>,
    data_receipt_links: HashMap<String, DataReceiptParentLink>,
    data_receipt_link_heights: BTreeMap<i64, Vec<String>>,
    closed_hos_since_refresh: usize,
    cleanup_resume_height: Option<i64>,
}

#[derive(Default)]
struct RecentBlockCache {
    shared_bundle: SourceBlockBundle,
}

#[derive(Clone)]
struct PendingGraphWarning {
    transaction_hash: String,
    has_hos_touch: bool,
    pending_receipt_count: usize,
    last_seen_height: i64,
}

#[derive(Default)]
struct StalledWarningSummary {
    total_count: usize,
    hos_touch_count: usize,
    max_pending_receipt_count: usize,
    oldest_last_seen_height: Option<i64>,
    sample_tx_hashes: Vec<String>,
}

#[derive(Default)]
struct BlockMergePlan {
    closed_hos: Vec<(String, HosGraphBundle)>,
    closed_raw_only: Vec<String>,
    pruned_non_hos: Vec<String>,
    dropped_incomplete_hos: Vec<(String, String)>,
    incomplete_graph_count: usize,
    active_graph_count: usize,
    active_hos_graph_count: usize,
    ready_hos_graph_count: usize,
    yield_open_graph_count: usize,
    late_first_touch_cache_miss_count: usize,
    max_open_graph_age_blocks: i64,
}

#[derive(Default)]
struct MaintenancePlan {
    triggered: bool,
    stalled_warning_summary: StalledWarningSummary,
    active_graph_tx_hashes: HashSet<String>,
}

#[derive(Default)]
struct RecentCacheEvictionStats {
    evicted_block_count: usize,
}

impl HosRuntimeState {
    fn set_cleanup_resume_height(&mut self, cleanup_resume_height: Option<i64>) {
        self.cleanup_resume_height = cleanup_resume_height;
    }

    fn should_skip_receipt_tx_map_cleanup(&mut self, block_height: i64) -> bool {
        let Some(cleanup_resume_height) = self.cleanup_resume_height else {
            return false;
        };

        if block_height <= cleanup_resume_height {
            return true;
        }

        self.cleanup_resume_height = None;
        false
    }

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

    fn merge_block_updates(
        &mut self,
        block_height: i64,
        updates: HosBlockUpdates,
        raw_enabled: bool,
        stats: &mut StageMetrics,
    ) -> BlockMergePlan {
        stats.record_count("block_write_rows", updates.row_count());
        let mut merge_metrics = StageMetrics::new();
        let HosBlockUpdates {
            receipt_tx_hash_map: _,
            data_receipt_links,
            graph_updates,
            shared_bundle,
        } = updates;
        merge_metrics.record_count("graph_update_count", graph_updates.len());
        merge_metrics.record_count("shared_bundle_row_count", shared_bundle.row_count());

        let remember_links_start = Instant::now();
        self.remember_data_receipt_links(block_height, data_receipt_links);
        merge_metrics.record_phase_elapsed("remember_data_receipt_links", remember_links_start);

        let update_loop_start = Instant::now();
        let mut hydrated_graph_count = 0_u64;
        let mut hydrated_row_count = 0_u64;
        let mut current_slice_graph_count = 0_u64;
        let mut current_slice_row_count = 0_u64;
        let mut recent_ref_graph_count = 0_u64;
        let mut became_hos_touch_count = 0_u64;
        for (transaction_hash, update) in graph_updates {
            let prior_hos_touch = self
                .graphs
                .get(&transaction_hash)
                .map(|graph| graph.has_hos_touch)
                .unwrap_or(false);
            let hydrated_bundle = if !raw_enabled && !prior_hos_touch && update.has_hos_touch {
                let hydrate_start = Instant::now();
                let bundle = self
                    .graphs
                    .get(&transaction_hash)
                    .map(|graph| self.hydrate_bundle_from_recent_cache(graph))
                    .unwrap_or_default();
                merge_metrics.record_phase_elapsed("hydrate_bundle", hydrate_start);
                hydrated_graph_count += 1;
                hydrated_row_count += bundle.row_count() as u64;
                bundle
            } else {
                HosGraphBundle::default()
            };
            let current_slice = if !raw_enabled && (prior_hos_touch || update.has_hos_touch) {
                let current_slice_start = Instant::now();
                let slice = Self::materialize_bundle_from_shared(&shared_bundle, &update.refs);
                merge_metrics
                    .record_phase_elapsed("materialize_current_slice", current_slice_start);
                current_slice_graph_count += 1;
                current_slice_row_count += slice.row_count() as u64;
                Some(slice)
            } else {
                None
            };

            let graph = self
                .graphs
                .entry(transaction_hash.clone())
                .or_insert_with(|| GraphState {
                    first_seen_height: block_height,
                    last_seen_height: block_height,
                    ..GraphState::default()
                });

            graph.first_seen_height = if graph.first_seen_height == 0 {
                block_height
            } else {
                graph.first_seen_height.min(block_height)
            };
            graph.last_seen_height = graph.last_seen_height.max(block_height);
            graph.has_hos_touch |= update.has_hos_touch;
            graph.root_transaction_seen |= update.root_transaction_seen;

            for receipt_id in update.pending_receipts_add {
                graph.pending_receipts.insert(receipt_id);
            }
            for receipt_id in update.pending_receipts_remove {
                graph.pending_receipts.remove(&receipt_id);
            }
            graph.open_yield_receipts.extend(update.yielded_receipts);
            graph.resolve_yield_receipts(update.resumed_yield_receipt_count);
            graph.expire_timed_out_yields(block_height);

            let became_hos_touch = !prior_hos_touch && graph.has_hos_touch;
            if !raw_enabled && graph.has_hos_touch {
                if became_hos_touch {
                    became_hos_touch_count += 1;
                }
                if became_hos_touch && graph.bundle.is_none() {
                    graph.bundle = Some(hydrated_bundle);
                    graph.recent_refs.clear();
                }

                let bundle = graph.bundle.get_or_insert_with(HosGraphBundle::default);
                if let Some(slice) = &current_slice {
                    bundle.merge(slice.clone());
                }

                if !graph.root_transaction_seen || !bundle.has_root_transaction(&transaction_hash) {
                    if graph.incomplete_reason.is_none() {
                        graph.incomplete_reason = Some("missing_root_transaction_row".to_string());
                    }
                } else if graph.incomplete_reason.as_deref() == Some("missing_root_transaction_row")
                {
                    graph.incomplete_reason = None;
                }
            } else if !raw_enabled && !update.refs.is_empty() {
                recent_ref_graph_count += 1;
                graph
                    .recent_refs
                    .entry(block_height)
                    .or_default()
                    .merge(update.refs);
            }
        }
        merge_metrics.record_phase_elapsed("graph_update_loop", update_loop_start);
        merge_metrics.record_count("hydrated_graph_count", hydrated_graph_count);
        merge_metrics.record_count("hydrated_row_count", hydrated_row_count);
        merge_metrics.record_count("current_slice_graph_count", current_slice_graph_count);
        merge_metrics.record_count("current_slice_row_count", current_slice_row_count);
        merge_metrics.record_count("recent_ref_graph_count", recent_ref_graph_count);
        merge_metrics.record_count("became_hos_touch_count", became_hos_touch_count);

        if !raw_enabled && shared_bundle.row_count() > 0 {
            let cache_insert_start = Instant::now();
            self.recent_blocks
                .insert(block_height, RecentBlockCache { shared_bundle });
            merge_metrics.record_phase_elapsed("recent_cache_insert", cache_insert_start);
        }

        let mut plan = BlockMergePlan::default();
        let mut remove_now = Vec::new();
        merge_metrics.record_count("closure_scan_graph_count", self.graphs.len());
        let closure_scan_start = Instant::now();

        for (transaction_hash, graph) in &mut self.graphs {
            graph.expire_timed_out_yields(block_height);
            if !graph.is_closed() {
                if graph.incomplete_reason.is_some() {
                    plan.incomplete_graph_count += 1;
                }
                plan.max_open_graph_age_blocks = plan
                    .max_open_graph_age_blocks
                    .max(graph.open_graph_age_blocks(block_height));
                if !graph.open_yield_receipts.is_empty() {
                    plan.yield_open_graph_count += 1;
                }
                continue;
            }

            if !graph.has_hos_touch {
                plan.pruned_non_hos.push(transaction_hash.clone());
                remove_now.push(transaction_hash.clone());
                continue;
            }

            if raw_enabled {
                plan.closed_raw_only.push(transaction_hash.clone());
                remove_now.push(transaction_hash.clone());
                continue;
            }

            let Some(bundle) = graph.bundle.clone() else {
                plan.dropped_incomplete_hos.push((
                    transaction_hash.clone(),
                    graph
                        .incomplete_reason
                        .clone()
                        .unwrap_or_else(|| "missing_bundle".to_string()),
                ));
                remove_now.push(transaction_hash.clone());
                continue;
            };

            if !graph.root_transaction_seen || !bundle.has_root_transaction(transaction_hash) {
                plan.dropped_incomplete_hos.push((
                    transaction_hash.clone(),
                    graph
                        .incomplete_reason
                        .clone()
                        .unwrap_or_else(|| "missing_root_transaction_row".to_string()),
                ));
                remove_now.push(transaction_hash.clone());
                continue;
            }

            if let Some(reason) = &graph.incomplete_reason {
                plan.dropped_incomplete_hos
                    .push((transaction_hash.clone(), reason.clone()));
                remove_now.push(transaction_hash.clone());
                continue;
            }

            plan.closed_hos.push((transaction_hash.clone(), bundle));
        }
        merge_metrics.record_phase_elapsed("closure_scan", closure_scan_start);
        merge_metrics.record_count("remove_now_count", remove_now.len());

        for tx_hash in remove_now {
            self.graphs.remove(&tx_hash);
        }

        if !raw_enabled {
            let cache_evict_start = Instant::now();
            let eviction_stats = self.evict_unreferenced_source_blocks();
            merge_metrics.record_phase_elapsed("recent_cache_evict", cache_evict_start);
            merge_metrics.record_count("evicted_block_count", eviction_stats.evicted_block_count);
        }

        plan.ready_hos_graph_count = plan.closed_hos.len();
        plan.active_graph_count = self.graphs.len();
        plan.active_hos_graph_count = self
            .graphs
            .values()
            .filter(|graph| graph.has_hos_touch)
            .count();

        stats.record_count("active_graph_count", plan.active_graph_count);
        stats.record_count("active_hos_graph_count", plan.active_hos_graph_count);
        stats.record_count("source_cache_block_count", self.recent_blocks.len());
        stats.record_count("recent_cache_block_count", self.recent_blocks.len());
        stats.merge_with_prefix("merge.", merge_metrics);

        plan
    }

    fn hydrate_bundle_from_recent_cache(&self, graph: &GraphState) -> HosGraphBundle {
        let mut bundle = HosGraphBundle::default();
        for (block_height, refs) in &graph.recent_refs {
            if let Some(block_cache) = self.recent_blocks.get(block_height) {
                bundle.merge(Self::materialize_bundle_from_shared(
                    &block_cache.shared_bundle,
                    refs,
                ));
            }
        }
        bundle
    }

    fn materialize_bundle_from_shared(
        shared_bundle: &SourceBlockBundle,
        refs: &TxSliceRefs,
    ) -> HosGraphBundle {
        let mut bundle = HosGraphBundle::default();

        if let Some(block_source) = shared_bundle.block.as_ref() {
            let block_height = block_source.block.header.height as i64;
            if refs.block_heights.contains(&block_height) {
                if let Ok(row) = VenearGovernanceModule::block_source_to_row(block_source) {
                    bundle.blocks.insert(block_height, row);
                }
            }
        }
        for chunk_hash in &refs.chunk_hashes {
            if let Some(row) = shared_bundle.chunks.get(chunk_hash) {
                if let Ok(materialized) = VenearGovernanceModule::chunk_source_to_row(row) {
                    bundle.chunks.insert(chunk_hash.clone(), materialized);
                }
            }
        }
        for key in &refs.state_change_keys {
            if let Some(row) = shared_bundle.state_changes.get(key) {
                if let Ok(materialized) = VenearGovernanceModule::state_changes_source_to_row(row) {
                    bundle.state_changes.insert(*key, materialized);
                }
            }
        }
        for hash in &refs.transaction_hashes {
            if let Some(row) = shared_bundle.transactions.get(hash) {
                if let Ok(materialized) = VenearGovernanceModule::transaction_source_to_row(row) {
                    bundle.transactions.insert(hash.clone(), materialized);
                }
            }
        }
        for receipt_id in &refs.receipt_ids {
            if let Some(row) = shared_bundle.receipts.get(receipt_id) {
                if let Ok(materialized) = VenearGovernanceModule::receipt_source_to_row(row) {
                    bundle.receipts.insert(receipt_id.clone(), materialized);
                }
            }
        }
        for outcome_id in &refs.execution_outcome_ids {
            if let Some(row) = shared_bundle.execution_outcomes.get(outcome_id) {
                if let Ok(materialized) =
                    VenearGovernanceModule::execution_outcome_source_to_row(row)
                {
                    bundle
                        .execution_outcomes
                        .insert(outcome_id.clone(), materialized);
                }
                if let Ok(receipt_actions) =
                    VenearGovernanceModule::receipt_action_rows_from_execution_outcome(row)
                {
                    for action_row in receipt_actions {
                        bundle
                            .receipt_actions
                            .insert(action_row.id.clone(), action_row);
                    }
                }
            }
        }
        for edge_key in &refs.receipt_edge_keys {
            if let Some(row) = shared_bundle.receipt_edges.get(edge_key) {
                bundle.receipt_edges.insert(edge_key.clone(), row.clone());
            }
        }

        bundle
    }

    fn evict_unreferenced_source_blocks(&mut self) -> RecentCacheEvictionStats {
        let mut stats = RecentCacheEvictionStats::default();

        let retained_heights = self
            .graphs
            .values()
            .flat_map(|graph| graph.recent_refs.keys().copied())
            .collect::<HashSet<_>>();

        let stale_heights = self
            .recent_blocks
            .keys()
            .copied()
            .filter(|height| !retained_heights.contains(height))
            .collect::<Vec<_>>();

        for height in stale_heights {
            if self.recent_blocks.remove(&height).is_some() {
                stats.evicted_block_count += 1;
            }
        }

        stats
    }

    fn collect_maintenance_plan(
        &self,
        block_height: i64,
        finalization_interval_blocks: i64,
    ) -> MaintenancePlan {
        let mut plan = MaintenancePlan {
            active_graph_tx_hashes: self.graphs.keys().cloned().collect(),
            ..MaintenancePlan::default()
        };

        if finalization_interval_blocks <= 0 || block_height % finalization_interval_blocks != 0 {
            return plan;
        }

        plan.triggered = true;
        let warning_threshold_height =
            block_height.saturating_sub(STALLED_PENDING_GRAPH_WARNING_BLOCKS.max(0));

        for (transaction_hash, graph) in &self.graphs {
            if !graph.is_closed()
                && (graph.has_hos_touch || graph.incomplete_reason.is_some())
                && graph.last_seen_height <= warning_threshold_height
            {
                let warning = PendingGraphWarning {
                    transaction_hash: transaction_hash.clone(),
                    has_hos_touch: graph.has_hos_touch,
                    pending_receipt_count: graph.pending_receipts.len()
                        + graph.open_yield_receipts.len(),
                    last_seen_height: graph.last_seen_height,
                };
                plan.stalled_warning_summary.total_count += 1;
                if warning.has_hos_touch {
                    plan.stalled_warning_summary.hos_touch_count += 1;
                }
                plan.stalled_warning_summary.max_pending_receipt_count = plan
                    .stalled_warning_summary
                    .max_pending_receipt_count
                    .max(warning.pending_receipt_count);
                plan.stalled_warning_summary.oldest_last_seen_height = Some(
                    plan.stalled_warning_summary
                        .oldest_last_seen_height
                        .map(|height| height.min(warning.last_seen_height))
                        .unwrap_or(warning.last_seen_height),
                );
                if plan.stalled_warning_summary.sample_tx_hashes.len() < 5 {
                    plan.stalled_warning_summary
                        .sample_tx_hashes
                        .push(warning.transaction_hash);
                }
            }
        }

        plan
    }

    fn remove_closed_hos_graphs(&mut self, tx_hashes: &[String]) {
        for tx_hash in tx_hashes {
            self.graphs.remove(tx_hash);
        }
    }

    fn note_closed_hos_graphs(&mut self, count: usize) {
        self.closed_hos_since_refresh += count;
    }

    fn take_mview_refresh_count(&mut self) -> usize {
        let pending = self.closed_hos_since_refresh;
        self.closed_hos_since_refresh = 0;
        pending
    }
}

#[derive(Default)]
struct CanonicalWriteBatch {
    blocks: HashMap<i64, NearBlockRow>,
    chunks: HashMap<String, NearChunkRow>,
    state_changes: HashMap<(i64, i64), NearStateChangesRow>,
    transactions: HashMap<String, NearTransactionRow>,
    receipts: HashMap<String, NearReceiptRow>,
    execution_outcomes: HashMap<String, NearExecutionOutcomeRow>,
    receipt_actions: HashMap<String, NearReceiptActionRow>,
    receipt_edges: HashMap<(String, String), NearReceiptEdgeRow>,
}

impl CanonicalWriteBatch {
    fn merge_bundle(&mut self, bundle: HosGraphBundle) {
        self.blocks.extend(bundle.blocks);
        self.chunks.extend(bundle.chunks);
        self.state_changes.extend(bundle.state_changes);
        self.transactions.extend(bundle.transactions);
        self.receipts.extend(bundle.receipts);
        self.execution_outcomes.extend(bundle.execution_outcomes);
        self.receipt_actions.extend(bundle.receipt_actions);
        self.receipt_edges.extend(bundle.receipt_edges);
    }

    fn row_count(&self) -> usize {
        self.blocks.len()
            + self.chunks.len()
            + self.state_changes.len()
            + self.transactions.len()
            + self.receipts.len()
            + self.execution_outcomes.len()
            + self.receipt_actions.len()
            + self.receipt_edges.len()
    }
}

pub struct VenearGovernanceModule {
    enabled: bool,
    stream_mode: NearModuleStreamMode,
    mview_refresh_interval_blocks: i64,
    finalization_interval_blocks: i64,
    restart_rewind_blocks: i64,
    receipt_tx_map_retention_blocks: i64,
    raw_enabled: bool,
    state: Mutex<HosRuntimeState>,
}

impl VenearGovernanceModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        let mut enabled = false;
        let mut stream_mode = NearModuleStreamMode::Shared;
        let mut mview_refresh_interval_blocks = DEFAULT_HOS_MVIEW_REFRESH_INTERVAL_BLOCKS;
        let mut finalization_interval_blocks = DEFAULT_FINALIZATION_INTERVAL_BLOCKS;
        let mut restart_rewind_blocks = 0_i64;
        let mut receipt_tx_map_retention_blocks = DEFAULT_TX_MAP_RETENTION_BLOCKS;
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
                        finalization_interval_blocks =
                            i64::try_from(module_cfg.finalization_interval_blocks)
                                .unwrap_or(i64::MAX);
                        restart_rewind_blocks =
                            i64::try_from(module_cfg.restart_rewind_blocks).unwrap_or(i64::MAX);
                        receipt_tx_map_retention_blocks =
                            i64::try_from(module_cfg.receipt_tx_map_retention_blocks)
                                .unwrap_or(i64::MAX);
                    }
                }
            }
        }

        Self {
            enabled,
            stream_mode,
            mview_refresh_interval_blocks,
            finalization_interval_blocks,
            restart_rewind_blocks,
            receipt_tx_map_retention_blocks,
            raw_enabled,
            state: Mutex::new(HosRuntimeState::default()),
        }
    }

    fn minimum_receipt_tx_map_retention_blocks(
        restart_rewind_blocks: i64,
        finalization_interval_blocks: i64,
    ) -> i64 {
        restart_rewind_blocks
            .max(0)
            .saturating_add(DEFAULT_YIELD_TIMEOUT_BLOCKS)
            .saturating_add(finalization_interval_blocks.max(0))
    }

    fn effective_receipt_tx_map_retention_blocks_for(
        configured_retention_blocks: i64,
        restart_rewind_blocks: i64,
        finalization_interval_blocks: i64,
    ) -> i64 {
        configured_retention_blocks.max(Self::minimum_receipt_tx_map_retention_blocks(
            restart_rewind_blocks,
            finalization_interval_blocks,
        ))
    }

    fn effective_receipt_tx_map_retention_blocks(&self) -> i64 {
        Self::effective_receipt_tx_map_retention_blocks_for(
            self.receipt_tx_map_retention_blocks,
            self.restart_rewind_blocks,
            self.finalization_interval_blocks,
        )
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

    fn block_source_to_row(source: &BlockSource) -> Result<NearBlockRow> {
        let block = source.block.as_ref();
        let header = &block.header;

        Ok(NearBlockRow {
            height: header.height as i64,
            hash: header.hash.to_string(),
            prev_hash: header.prev_hash.to_string(),
            epoch_id: header.epoch_id.to_string(),
            next_epoch_id: header.next_epoch_id.to_string(),
            author: source.author.clone(),
            timestamp: header.timestamp as i64,
            timestamp_nanosec: header.timestamp_nanosec as i64,
            gas_price: header.gas_price.to_string(),
            total_supply: header.total_supply.to_string(),
            validator_reward: header.validator_reward.to_string(),
            protocol_version: header.latest_protocol_version as i32,
            chunks_included: header.chunks_included as i64,
            block_ordinal: header.block_ordinal.map(|value| value as i64),
            prev_height: header.prev_height.map(|value| value as i64),
            raw_data: Self::serialize_json(block, "block raw_data")?,
        })
    }

    fn chunk_source_to_row(source: &ChunkSource) -> Result<NearChunkRow> {
        let chunk = source.chunk.as_ref();
        let header = &chunk.header;

        Ok(NearChunkRow {
            chunk_hash: header.chunk_hash.to_string(),
            block_height: source.block_height,
            shard_id: source.shard_id,
            author: chunk.author.to_string(),
            height_created: header.height_created as i64,
            height_included: header.height_included as i64,
            gas_used: header.gas_used.as_gas() as i64,
            gas_limit: header.gas_limit.as_gas() as i64,
            validator_reward: header.validator_reward.to_string(),
            raw_data: Self::serialize_json(chunk, "chunk raw_data")?,
        })
    }

    fn state_changes_source_to_row(source: &StateChangesSource) -> Result<NearStateChangesRow> {
        Ok(NearStateChangesRow {
            block_height: source.block_height,
            shard_id: source.shard_id,
            state_changes: Self::serialize_json(
                source.state_changes.as_ref(),
                "state changes payload",
            )?,
        })
    }

    fn transaction_source_to_row(source: &TransactionSource) -> Result<NearTransactionRow> {
        let transaction = source.transaction.as_ref();

        Ok(NearTransactionRow {
            hash: transaction.hash.to_string(),
            block_height: source.block_height,
            shard_id: source.shard_id,
            signer_id: transaction.signer_id.to_string(),
            receiver_id: transaction.receiver_id.to_string(),
            nonce: transaction.nonce as i64,
            priority_fee: transaction.priority_fee as i64,
            public_key: transaction.public_key.to_string(),
            signature: transaction.signature.to_string(),
            actions: Self::serialize_json(&transaction.actions, "HoS transaction actions")?,
            raw_data: Self::serialize_json(transaction, "HoS transaction raw_data")?,
        })
    }

    fn receipt_source_to_row(source: &ReceiptSource) -> Result<NearReceiptRow> {
        let receipt = source.receipt.as_ref();

        Ok(NearReceiptRow {
            receipt_id: receipt.receipt_id.to_string(),
            block_height: source.block_height,
            shard_id: source.shard_id,
            predecessor_id: receipt.predecessor_id.to_string(),
            receiver_id: receipt.receiver_id.to_string(),
            receipt: Self::serialize_json(&receipt.receipt, "receipt body")?,
            raw_data: Self::serialize_json(receipt, "receipt row")?,
        })
    }

    fn execution_outcome_source_to_row(
        source: &ExecutionOutcomeSource,
    ) -> Result<NearExecutionOutcomeRow> {
        let outcome_view = source.execution_outcome.as_ref();
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

        Ok(NearExecutionOutcomeRow {
            id: source.id.clone(),
            transaction_hash: source.transaction_hash.clone(),
            block_height: source.block_height,
            shard_id: source.shard_id,
            receipt_id: source.receipt_id.clone(),
            outcome: Self::serialize_json(&outcome_view.outcome, "execution outcome")?,
            raw_data: Self::serialize_json(outcome_view, "execution outcome row")?,
            block_hash: source.block_hash.clone(),
            chunk_hash: source.chunk_hash.clone(),
            gas_burnt: outcome.gas_burnt.as_gas() as i64,
            gas_used: outcome.gas_burnt.as_gas() as f64,
            tokens_burnt: outcome.tokens_burnt.as_yoctonear() as f64 / 1e24,
            executor_account_id: outcome.executor_id.to_string(),
            status: status.to_string(),
            outcome_receipt_ids,
            executed_in_block_hash: source.block_hash.clone(),
            logs,
            results_json,
            block_timestamp: source.block_timestamp,
        })
    }

    fn receipt_action_rows_from_execution_outcome(
        source: &ExecutionOutcomeSource,
    ) -> Result<Vec<NearReceiptActionRow>> {
        let Some(receipt) = source.receipt.as_ref() else {
            return Ok(Vec::new());
        };

        let ReceiptEnumView::Action {
            signer_id,
            signer_public_key,
            gas_price,
            actions,
            ..
        } = &receipt.receipt
        else {
            return Ok(Vec::new());
        };

        let mut rows = Vec::with_capacity(actions.len());
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

            rows.push(NearReceiptActionRow {
                id: format!("{}-{action_index}", receipt.receipt_id),
                transaction_hash: source.transaction_hash.clone(),
                block_height: source.block_height,
                receipt_id: receipt.receipt_id.to_string(),
                execution_outcome_id: source.id.clone(),
                shard_id: source.shard_id,
                signer_account_id: Some(signer_id_value),
                signer_public_key: Some(signer_public_key_value),
                gas_price: Some(gas_price_value),
                action_kind,
                action_json,
                predecessor_id: receipt.predecessor_id.to_string(),
                receiver_id: receipt.receiver_id.to_string(),
                block_hash: source.block_hash.clone(),
                chunk_hash: source.chunk_hash.clone(),
                author: source.author.clone(),
                method_name,
                gas,
                deposit,
                args_base64,
                args_json,
                action_index: action_index as i32,
                block_timestamp: source.block_timestamp,
            });
        }

        Ok(rows)
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

    fn is_hos_related_receipt(&self, receipt: &ReceiptView) -> bool {
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

    fn is_promise_yield(receipt: &ReceiptEnumView) -> bool {
        matches!(
            receipt,
            ReceiptEnumView::Action {
                is_promise_yield: true,
                ..
            }
        )
    }

    fn is_promise_resume(receipt: &ReceiptEnumView) -> bool {
        matches!(
            receipt,
            ReceiptEnumView::Data {
                is_promise_resume: true,
                ..
            }
        )
    }

    fn data_receipt_link(
        links: &HashMap<String, DataReceiptParentLink>,
        receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
    ) -> Option<DataReceiptParentLink> {
        let ReceiptEnumView::Data { data_id, .. } = &receipt.receipt else {
            return None;
        };
        links.get(&data_id.to_string()).cloned()
    }

    fn cache_output_data_receivers(
        links: &mut HashMap<String, DataReceiptParentLink>,
        receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
        transaction_hash: &str,
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
                    transaction_hash: transaction_hash.to_string(),
                    parent_receipt_id: receipt.receipt_id.to_string(),
                    parent_execution_outcome_id: parent_execution_outcome_id.to_string(),
                    block_height,
                    block_hash: block_hash.to_string(),
                    block_timestamp,
                });
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

    fn take_pending_mview_refresh(&self, block_height: i64) -> (bool, usize) {
        if !self.should_refresh_mviews(block_height) {
            return (false, 0);
        }

        let mut state = self.state.lock().unwrap();
        let pending = state.take_mview_refresh_count();
        (pending > 0, pending)
    }

    async fn flush_canonical_transactions(
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
            "failed to bulk upsert near_transactions",
        )
        .await
    }

    async fn flush_canonical_receipts(
        db: &impl ConnectionTrait,
        rows: &[NearReceiptRow],
    ) -> Result<()> {
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
            ON CONFLICT (receipt_id) DO UPDATE SET
                block_height = EXCLUDED.block_height,
                shard_id = EXCLUDED.shard_id,
                predecessor_id = EXCLUDED.predecessor_id,
                receiver_id = EXCLUDED.receiver_id,
                receipt = EXCLUDED.receipt,
                raw_data = COALESCE(near.near_receipts.raw_data, EXCLUDED.raw_data)",
            |row, values| {
                values.push(row.receipt_id.clone().into());
                values.push(row.block_height.into());
                values.push(row.shard_id.into());
                values.push(row.predecessor_id.clone().into());
                values.push(row.receiver_id.clone().into());
                values.push(row.receipt.clone().into());
                values.push(row.raw_data.clone().into());
            },
            "failed to bulk upsert near_receipts",
        )
        .await
    }

    async fn flush_canonical_execution_outcomes(
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

    async fn flush_canonical_receipt_actions(
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

    async fn flush_canonical_receipt_edges(
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

    async fn flush_canonical_blocks(
        db: &impl ConnectionTrait,
        rows: &[NearBlockRow],
    ) -> Result<()> {
        Self::execute_chunked_upsert(
            db,
            rows,
            16,
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
            ) VALUES ",
            "
            ON CONFLICT (height) DO NOTHING",
            |row, values| {
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
            },
            "failed to bulk insert near_blocks",
        )
        .await
    }

    async fn flush_canonical_chunks(
        db: &impl ConnectionTrait,
        rows: &[NearChunkRow],
    ) -> Result<()> {
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

    async fn flush_canonical_state_changes(
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

    async fn flush_canonical_write_batch(
        db: &impl ConnectionTrait,
        batch: CanonicalWriteBatch,
        stats: &mut StageMetrics,
    ) -> Result<()> {
        stats.record_count("canonical_write_rows", batch.row_count());

        let transactions = batch.transactions.into_values().collect::<Vec<_>>();
        let receipts = batch.receipts.into_values().collect::<Vec<_>>();
        let execution_outcomes = batch.execution_outcomes.into_values().collect::<Vec<_>>();
        let receipt_actions = batch.receipt_actions.into_values().collect::<Vec<_>>();
        let receipt_edges = batch.receipt_edges.into_values().collect::<Vec<_>>();
        let blocks = batch.blocks.into_values().collect::<Vec<_>>();
        let chunks = batch.chunks.into_values().collect::<Vec<_>>();
        let state_changes = batch.state_changes.into_values().collect::<Vec<_>>();

        let start = Instant::now();
        Self::flush_canonical_transactions(db, &transactions).await?;
        stats.record_phase_elapsed("canonical_transaction_write", start);

        let start = Instant::now();
        Self::flush_canonical_receipts(db, &receipts).await?;
        stats.record_phase_elapsed("canonical_receipt_write", start);

        let start = Instant::now();
        Self::flush_canonical_execution_outcomes(db, &execution_outcomes).await?;
        stats.record_phase_elapsed("canonical_execution_outcome_write", start);

        let start = Instant::now();
        Self::flush_canonical_receipt_actions(db, &receipt_actions).await?;
        stats.record_phase_elapsed("canonical_receipt_action_write", start);

        let start = Instant::now();
        Self::flush_canonical_receipt_edges(db, &receipt_edges).await?;
        stats.record_phase_elapsed("canonical_receipt_edge_write", start);

        let start = Instant::now();
        Self::flush_canonical_blocks(db, &blocks).await?;
        stats.record_phase_elapsed("canonical_block_write", start);

        let start = Instant::now();
        Self::flush_canonical_chunks(db, &chunks).await?;
        stats.record_phase_elapsed("canonical_chunk_write", start);

        let start = Instant::now();
        Self::flush_canonical_state_changes(db, &state_changes).await?;
        stats.record_phase_elapsed("canonical_state_changes_write", start);

        Ok(())
    }

    async fn run_receipt_tx_map_cleanup(
        &self,
        db: &impl ConnectionTrait,
        block_height: i64,
        active_graph_tx_hashes: &HashSet<String>,
    ) -> Result<u64> {
        let cutoff_height =
            block_height.saturating_sub(self.effective_receipt_tx_map_retention_blocks());
        let mut deleted_total = 0_u64;

        for _ in 0..RECEIPT_TX_MAP_CLEANUP_MAX_BATCHES {
            let mut values = vec![cutoff_height.into()];
            let mut sql = String::from(
                "WITH deletable AS (
                    SELECT m.receipt_id
                    FROM near.near_receipt_tx_hash_map m
                    WHERE m.last_seen_height < $1",
            );

            if !active_graph_tx_hashes.is_empty() {
                sql.push_str(" AND m.transaction_hash NOT IN (");
                let start_idx = values.len() + 1;
                let mut first = true;
                for (idx, tx_hash) in active_graph_tx_hashes.iter().enumerate() {
                    if !first {
                        sql.push_str(", ");
                    }
                    first = false;
                    sql.push('$');
                    sql.push_str(&(start_idx + idx).to_string());
                    values.push(tx_hash.clone().into());
                }
                sql.push(')');
            }

            let batch_placeholder = values.len() + 1;
            values.push((RECEIPT_TX_MAP_CLEANUP_BATCH_SIZE as i64).into());
            sql.push_str(&format!(
                "
                    ORDER BY m.last_seen_height ASC
                    LIMIT ${batch_placeholder}
                 )
                 DELETE FROM near.near_receipt_tx_hash_map m
                 USING deletable d
                 WHERE m.receipt_id = d.receipt_id"
            ));

            let result = db
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    &sql,
                    values,
                ))
                .await
                .context("failed running near_receipt_tx_hash_map cleanup")?;

            let affected = result.rows_affected();
            deleted_total += affected;
            if affected == 0 {
                break;
            }
        }

        Ok(deleted_total)
    }

    fn note_common_block_data(
        updates: &mut HosBlockUpdates,
        transaction_hash: &str,
        block_height: i64,
    ) {
        updates.note_block_ref(transaction_hash, block_height);
    }

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
        let block_source = BlockSource {
            author: default_author.clone(),
            block: Arc::new(msg.block.clone()),
        };

        let mut receipt_tx_hashes = build_receipt_tx_hashes(&msg.shards);
        let mut data_receipt_links = {
            let state = self.state.lock().unwrap();
            state.data_receipt_links.clone()
        };
        let mut lookup_receipt_ids = Vec::new();

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
        let mapped_receipts = load_receipt_tx_hash_map(db, &lookup_receipt_ids).await?;
        for (receipt_id, transaction_hash) in mapped_receipts {
            receipt_tx_hashes
                .entry(receipt_id)
                .or_insert_with(|| transaction_hash);
        }
        stats.record_phase_elapsed("lineage_lookup", lineage_lookup_start);

        let mut updates = HosBlockUpdates::default();
        updates.set_block_source(block_source);
        let mut touched_tx_hashes = HashSet::new();
        let mut unresolved_hos_receipts = Vec::new();

        let output_data_link_build_start = Instant::now();
        for shard in &msg.shards {
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    let Some(receipt) = &tx_with_outcome.outcome.receipt else {
                        continue;
                    };
                    let tx_hash = tx_with_outcome.transaction.hash.to_string();
                    let tx_outcome_id = tx_with_outcome.outcome.execution_outcome.id.to_string();
                    updates.note_output_data_receivers(
                        receipt,
                        &tx_hash,
                        &tx_outcome_id,
                        block_height,
                        &block_hash,
                        block_timestamp,
                    );
                    Self::cache_output_data_receivers(
                        &mut data_receipt_links,
                        receipt,
                        &tx_hash,
                        &tx_outcome_id,
                        block_height,
                        &block_hash,
                        block_timestamp,
                    );
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                let receipt_id = receipt.receipt_id.to_string();
                let data_receipt_link = Self::data_receipt_link(&data_receipt_links, receipt);
                let mut transaction_hash = receipt_tx_hashes.get(&receipt_id).cloned();
                if transaction_hash.is_none() {
                    transaction_hash = data_receipt_link
                        .as_ref()
                        .map(|link| link.transaction_hash.clone());
                }
                let Some(transaction_hash) = transaction_hash else {
                    continue;
                };
                let outcome_id = outcome_with_receipt.execution_outcome.id.to_string();

                updates.note_output_data_receivers(
                    receipt,
                    &transaction_hash,
                    &outcome_id,
                    block_height,
                    &block_hash,
                    block_timestamp,
                );
                Self::cache_output_data_receivers(
                    &mut data_receipt_links,
                    receipt,
                    &transaction_hash,
                    &outcome_id,
                    block_height,
                    &block_hash,
                    block_timestamp,
                );
            }
        }
        stats.record_phase_elapsed("output_data_link_build", output_data_link_build_start);

        let block_update_build_start = Instant::now();
        for shard in &msg.shards {
            let mut shard_touched_tx_hashes = HashSet::new();
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
                    updates.note_tx_metadata(&tx_hash, tx_root_related, true);
                    Self::note_common_block_data(&mut updates, &tx_hash, block_height);
                    updates.note_transaction(
                        &tx_hash,
                        TransactionSource {
                            block_height,
                            shard_id,
                            transaction: Arc::new(tx_with_outcome.transaction.clone()),
                        },
                    );

                    let tx_outcome_receipt_id = tx_with_outcome
                        .outcome
                        .receipt
                        .as_ref()
                        .map(|receipt| receipt.receipt_id.to_string());
                    let tx_outcome_view = &tx_with_outcome.outcome.execution_outcome;
                    let tx_outcome = &tx_outcome_view.outcome;
                    let tx_outcome_id = tx_outcome_view.id.to_string();
                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        let receipt_arc = Arc::new(receipt.clone());
                        let receipt_id = receipt.receipt_id.to_string();
                        receipt_tx_hashes.insert(receipt_id.clone(), tx_hash.clone());
                        updates.note_receipt_tx_hash_mapping(&receipt_id, &tx_hash, block_height);

                        let is_hos_root_related = self.is_hos_related_receipt(receipt);
                        if is_hos_root_related {
                            updates.note_tx_metadata(&tx_hash, true, false);
                        }

                        updates.note_receipt(
                            &tx_hash,
                            ReceiptSource {
                                block_height,
                                shard_id,
                                receipt: receipt_arc.clone(),
                            },
                        );
                        updates.note_executed_receipt(&tx_hash, &receipt_id);
                        if Self::is_promise_yield(&receipt.receipt) {
                            updates.note_yielded_receipt(
                                &tx_hash,
                                &receipt_id,
                                block_height.saturating_add(DEFAULT_YIELD_TIMEOUT_BLOCKS),
                            );
                        }
                        updates.note_output_data_receivers(
                            receipt,
                            &tx_hash,
                            &tx_outcome_id,
                            block_height,
                            &block_hash,
                            block_timestamp,
                        );
                        Self::cache_output_data_receivers(
                            &mut data_receipt_links,
                            receipt,
                            &tx_hash,
                            &tx_outcome_id,
                            block_height,
                            &block_hash,
                            block_timestamp,
                        );
                    }
                    updates.note_execution_outcome(
                        &tx_hash,
                        ExecutionOutcomeSource {
                            id: tx_outcome_id.clone(),
                            transaction_hash: tx_hash.clone(),
                            block_height,
                            shard_id,
                            receipt_id: tx_outcome_receipt_id.clone(),
                            execution_outcome: Arc::new(tx_outcome_view.clone()),
                            block_hash: block_hash.clone(),
                            chunk_hash: chunk_hash.clone(),
                            author: chunk_author.clone(),
                            block_timestamp,
                            receipt: tx_with_outcome
                                .outcome
                                .receipt
                                .as_ref()
                                .map(|receipt| Arc::new(receipt.clone())),
                        },
                    );

                    if let Some(parent_receipt_id) = tx_outcome_receipt_id.as_deref() {
                        for child_receipt_id in &tx_outcome.receipt_ids {
                            let child_receipt_id = child_receipt_id.to_string();
                            receipt_tx_hashes.insert(child_receipt_id.clone(), tx_hash.clone());
                            updates.note_receipt_tx_hash_mapping(
                                &child_receipt_id,
                                &tx_hash,
                                block_height,
                            );
                            updates.note_receipt_edge(
                                &tx_hash,
                                NearReceiptEdgeRow {
                                    parent_receipt_id: parent_receipt_id.to_string(),
                                    child_receipt_id: child_receipt_id.clone(),
                                    transaction_hash: tx_hash.clone(),
                                    parent_execution_outcome_id: tx_outcome_id.clone(),
                                    block_height,
                                    block_hash: block_hash.clone(),
                                    block_timestamp,
                                },
                            );
                            updates.note_pending_receipt(&tx_hash, &child_receipt_id);
                        }
                    } else {
                        for child_receipt_id in &tx_outcome.receipt_ids {
                            let child_receipt_id = child_receipt_id.to_string();
                            receipt_tx_hashes.insert(child_receipt_id.clone(), tx_hash.clone());
                            updates.note_receipt_tx_hash_mapping(
                                &child_receipt_id,
                                &tx_hash,
                                block_height,
                            );
                            updates.note_pending_receipt(&tx_hash, &child_receipt_id);
                        }
                    }
                }
            }

            if let Some(chunk) = &shard.chunk {
                for receipt in chunk.receipts.iter().chain(chunk.local_receipts.iter()) {
                    let receipt_id = receipt.receipt_id.to_string();
                    let data_receipt_link = Self::data_receipt_link(&data_receipt_links, receipt);
                    let mut transaction_hash = receipt_tx_hashes.get(&receipt_id).cloned();
                    if transaction_hash.is_none() {
                        transaction_hash = data_receipt_link
                            .as_ref()
                            .map(|link| link.transaction_hash.clone());
                    }
                    let is_hos_root_related = self.is_hos_related_receipt(receipt);

                    let Some(transaction_hash) = transaction_hash else {
                        if is_hos_root_related {
                            unresolved_hos_receipts.push(receipt_id);
                        }
                        continue;
                    };

                    touched_tx_hashes.insert(transaction_hash.clone());
                    shard_touched_tx_hashes.insert(transaction_hash.clone());
                    updates.note_tx_metadata(&transaction_hash, is_hos_root_related, false);
                    receipt_tx_hashes.insert(receipt_id.clone(), transaction_hash.clone());
                    updates.note_receipt_tx_hash_mapping(
                        &receipt_id,
                        &transaction_hash,
                        block_height,
                    );
                    if let Some(link) = data_receipt_link.as_ref() {
                        updates.note_receipt_edge(
                            &transaction_hash,
                            NearReceiptEdgeRow {
                                parent_receipt_id: link.parent_receipt_id.clone(),
                                child_receipt_id: receipt_id.clone(),
                                transaction_hash: transaction_hash.clone(),
                                parent_execution_outcome_id: link
                                    .parent_execution_outcome_id
                                    .clone(),
                                block_height: link.block_height,
                                block_hash: link.block_hash.clone(),
                                block_timestamp: link.block_timestamp,
                            },
                        );
                    }
                    Self::note_common_block_data(&mut updates, &transaction_hash, block_height);
                    updates.note_receipt(
                        &transaction_hash,
                        ReceiptSource {
                            block_height,
                            shard_id,
                            receipt: Arc::new(receipt.clone()),
                        },
                    );
                    if !matches!(receipt.receipt, ReceiptEnumView::Data { .. }) {
                        updates.note_pending_receipt(&transaction_hash, &receipt_id);
                    }
                    if Self::is_promise_yield(&receipt.receipt) {
                        updates.note_yielded_receipt(
                            &transaction_hash,
                            &receipt_id,
                            block_height.saturating_add(DEFAULT_YIELD_TIMEOUT_BLOCKS),
                        );
                    }
                    if Self::is_promise_resume(&receipt.receipt) {
                        updates.note_resumed_yield_receipt(&transaction_hash);
                    }
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                let receipt_id = receipt.receipt_id.to_string();
                let data_receipt_link = Self::data_receipt_link(&data_receipt_links, receipt);
                let mut transaction_hash = receipt_tx_hashes.get(&receipt_id).cloned();
                if transaction_hash.is_none() {
                    transaction_hash = data_receipt_link
                        .as_ref()
                        .map(|link| link.transaction_hash.clone());
                }
                let is_hos_root_related = self.is_hos_related_receipt(receipt);

                let Some(transaction_hash) = transaction_hash else {
                    if is_hos_root_related {
                        unresolved_hos_receipts.push(receipt_id);
                    }
                    continue;
                };

                receipt_tx_hashes.insert(receipt_id.clone(), transaction_hash.clone());
                touched_tx_hashes.insert(transaction_hash.clone());
                shard_touched_tx_hashes.insert(transaction_hash.clone());
                updates.note_tx_metadata(&transaction_hash, is_hos_root_related, false);
                updates.note_receipt_tx_hash_mapping(&receipt_id, &transaction_hash, block_height);
                if let Some(link) = data_receipt_link.as_ref() {
                    updates.note_receipt_edge(
                        &transaction_hash,
                        NearReceiptEdgeRow {
                            parent_receipt_id: link.parent_receipt_id.clone(),
                            child_receipt_id: receipt_id.clone(),
                            transaction_hash: transaction_hash.clone(),
                            parent_execution_outcome_id: link.parent_execution_outcome_id.clone(),
                            block_height: link.block_height,
                            block_hash: link.block_hash.clone(),
                            block_timestamp: link.block_timestamp,
                        },
                    );
                }
                Self::note_common_block_data(&mut updates, &transaction_hash, block_height);

                updates.note_receipt(
                    &transaction_hash,
                    ReceiptSource {
                        block_height,
                        shard_id,
                        receipt: Arc::new(receipt.clone()),
                    },
                );
                updates.note_executed_receipt(&transaction_hash, &receipt_id);
                if Self::is_promise_yield(&receipt.receipt) {
                    updates.note_yielded_receipt(
                        &transaction_hash,
                        &receipt_id,
                        block_height.saturating_add(DEFAULT_YIELD_TIMEOUT_BLOCKS),
                    );
                }
                if Self::is_promise_resume(&receipt.receipt) {
                    updates.note_resumed_yield_receipt(&transaction_hash);
                }

                let outcome_view = &outcome_with_receipt.execution_outcome;
                let outcome = &outcome_view.outcome;
                let outcome_id = outcome_view.id.to_string();
                updates.note_execution_outcome(
                    &transaction_hash,
                    ExecutionOutcomeSource {
                        id: outcome_id.clone(),
                        transaction_hash: transaction_hash.clone(),
                        block_height,
                        shard_id,
                        receipt_id: Some(receipt_id.clone()),
                        execution_outcome: Arc::new(outcome_view.clone()),
                        block_hash: block_hash.clone(),
                        chunk_hash: chunk_hash.clone(),
                        author: chunk_author.clone(),
                        block_timestamp,
                        receipt: Some(Arc::new(receipt.clone())),
                    },
                );

                for child_receipt_id in &outcome.receipt_ids {
                    let child_receipt_id = child_receipt_id.to_string();
                    receipt_tx_hashes.insert(child_receipt_id.clone(), transaction_hash.clone());
                    updates.note_receipt_tx_hash_mapping(
                        &child_receipt_id,
                        &transaction_hash,
                        block_height,
                    );
                    updates.note_receipt_edge(
                        &transaction_hash,
                        NearReceiptEdgeRow {
                            parent_receipt_id: receipt_id.clone(),
                            child_receipt_id: child_receipt_id.clone(),
                            transaction_hash: transaction_hash.clone(),
                            parent_execution_outcome_id: outcome_id.clone(),
                            block_height,
                            block_hash: block_hash.clone(),
                            block_timestamp,
                        },
                    );
                    updates.note_pending_receipt(&transaction_hash, &child_receipt_id);
                }
            }

            if let Some(chunk) = &shard.chunk {
                let chunk_source = ChunkSource {
                    block_height,
                    shard_id,
                    chunk: Arc::new(chunk.clone()),
                };
                for transaction_hash in &shard_touched_tx_hashes {
                    updates.note_chunk(transaction_hash, chunk_source.clone());
                }
            }

            if !shard.state_changes.is_empty() && !shard_touched_tx_hashes.is_empty() {
                let state_changes_source = StateChangesSource {
                    block_height,
                    shard_id,
                    state_changes: Arc::new(shard.state_changes.clone()),
                };
                for transaction_hash in &shard_touched_tx_hashes {
                    updates.note_state_changes(transaction_hash, state_changes_source.clone());
                }
            }
        }
        stats.record_phase_elapsed("block_update_build", block_update_build_start);

        stats.record_count("touched_tx_count", touched_tx_hashes.len());
        stats.record_count(
            "unresolved_hos_receipt_count",
            unresolved_hos_receipts.len(),
        );

        let receipt_map_rows = updates
            .receipt_tx_hash_map
            .values()
            .cloned()
            .collect::<Vec<_>>();
        stats.record_count("receipt_tx_map_write_rows", receipt_map_rows.len());
        let receipt_map_write_start = Instant::now();
        Self::flush_receipt_tx_hash_map_upserts(db, &receipt_map_rows).await?;
        stats.record_phase_elapsed("receipt_tx_map_write", receipt_map_write_start);

        let memory_merge_start = Instant::now();
        let merge_plan = {
            let mut state = self.state.lock().unwrap();
            state.merge_block_updates(block_height, updates, self.raw_enabled, &mut stats)
        };
        stats.record_phase_elapsed("memory_merge", memory_merge_start);

        stats.record_count("closed_hos_graph_count", merge_plan.closed_hos.len());
        stats.record_count(
            "closed_raw_only_graph_count",
            merge_plan.closed_raw_only.len(),
        );
        stats.record_count(
            "pruned_non_hos_graph_count",
            merge_plan.pruned_non_hos.len(),
        );
        stats.record_count("ready_hos_graph_count", merge_plan.ready_hos_graph_count);
        stats.record_count("yield_open_graph_count", merge_plan.yield_open_graph_count);
        stats.record_count(
            "late_first_touch_cache_miss_count",
            merge_plan.late_first_touch_cache_miss_count,
        );
        stats.record_count(
            "max_open_graph_age_blocks",
            merge_plan.max_open_graph_age_blocks,
        );
        stats.record_count("incomplete_graph_count", merge_plan.incomplete_graph_count);

        for (transaction_hash, reason) in &merge_plan.dropped_incomplete_hos {
            warn!(
                module = MODULE_NAME,
                transaction_hash = %transaction_hash,
                height = block_height,
                reason = %reason,
                "Dropping closed HoS graph without canonical persistence"
            );
        }

        if !self.raw_enabled && !merge_plan.closed_hos.is_empty() {
            let mut canonical_batch = CanonicalWriteBatch::default();
            for (_, bundle) in &merge_plan.closed_hos {
                canonical_batch.merge_bundle(bundle.clone());
            }

            let canonical_close_write_start = Instant::now();
            Self::flush_canonical_write_batch(db, canonical_batch, &mut stats).await?;
            stats.record_phase_elapsed("canonical_close_write", canonical_close_write_start);

            let closed_tx_hashes = merge_plan
                .closed_hos
                .iter()
                .map(|(transaction_hash, _)| transaction_hash.clone())
                .collect::<Vec<_>>();
            let mut state = self.state.lock().unwrap();
            state.remove_closed_hos_graphs(&closed_tx_hashes);
            state.note_closed_hos_graphs(closed_tx_hashes.len());
            stats.record_count("active_graph_count_after_close_write", state.graphs.len());
        } else if !merge_plan.closed_raw_only.is_empty() {
            let mut state = self.state.lock().unwrap();
            state.note_closed_hos_graphs(merge_plan.closed_raw_only.len());
        }

        let maintenance_plan = {
            let state = self.state.lock().unwrap();
            state.collect_maintenance_plan(block_height, self.finalization_interval_blocks)
        };

        if maintenance_plan.stalled_warning_summary.total_count > 0 {
            warn!(
                module = MODULE_NAME,
                stalled_graph_count = maintenance_plan.stalled_warning_summary.total_count,
                stalled_hos_graph_count = maintenance_plan.stalled_warning_summary.hos_touch_count,
                max_pending_receipt_count = maintenance_plan
                    .stalled_warning_summary
                    .max_pending_receipt_count,
                oldest_last_seen_height = maintenance_plan
                    .stalled_warning_summary
                    .oldest_last_seen_height,
                oldest_age_blocks = maintenance_plan
                    .stalled_warning_summary
                    .oldest_last_seen_height
                    .map(|height| block_height.saturating_sub(height))
                    .unwrap_or(0),
                sample_tx_hashes = ?maintenance_plan.stalled_warning_summary.sample_tx_hashes,
                "Stalled pending tx graphs remain unresolved; preserving in-memory state"
            );
        }

        stats.record_flag("maintenance_triggered", maintenance_plan.triggered);
        stats.record_count(
            "stalled_graph_count",
            maintenance_plan.stalled_warning_summary.total_count,
        );
        stats.record_count(
            "stalled_hos_graph_count",
            maintenance_plan.stalled_warning_summary.hos_touch_count,
        );
        stats.record_count(
            "effective_receipt_tx_map_retention_blocks",
            self.effective_receipt_tx_map_retention_blocks().max(0) as u64,
        );
        stats.record_flag("receipt_tx_map_cleanup_skipped_for_replay", false);

        if maintenance_plan.triggered {
            let finalize_start = Instant::now();
            let cleanup_start = Instant::now();
            let cleanup_skipped_for_replay = {
                let mut state = self.state.lock().unwrap();
                state.should_skip_receipt_tx_map_cleanup(block_height)
            };
            stats.record_flag(
                "receipt_tx_map_cleanup_skipped_for_replay",
                cleanup_skipped_for_replay,
            );
            let deleted = if cleanup_skipped_for_replay {
                0
            } else {
                self.run_receipt_tx_map_cleanup(
                    db,
                    block_height,
                    &maintenance_plan.active_graph_tx_hashes,
                )
                .await?
            };
            stats.record_phase_elapsed("receipt_tx_map_cleanup", cleanup_start);
            stats.record_count("receipt_tx_map_deleted", deleted);
            stats.record_phase_elapsed("maintenance_finalize", finalize_start);
        }

        for receipt_id in unresolved_hos_receipts {
            warn!(
                module = MODULE_NAME,
                receipt_id = %receipt_id,
                height = block_height,
                "HoS-related receipt observed without resolvable transaction hash; preserving nothing until replay or later lineage resolution"
            );
        }

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
        let (mview_refresh_triggered, mview_refresh_graph_count) =
            self.take_pending_mview_refresh(block_height);
        process_log
            .metrics_mut()
            .record_flag("mview_refresh_triggered", mview_refresh_triggered);
        process_log
            .metrics_mut()
            .record_count("mview_refresh_graph_count", mview_refresh_graph_count);

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

    async fn initialize(&self, db: &DatabaseConnection) -> Result<()> {
        let cleanup_resume_height = if self.restart_rewind_blocks > 0 {
            cursor::get_module_cursor(db, MODULE_NAME).await?
        } else {
            None
        };

        let mut state = self.state.lock().unwrap();
        state.set_cleanup_resume_height(cleanup_resume_height);

        if let Some(cleanup_resume_height) = cleanup_resume_height {
            info!(
                module = MODULE_NAME,
                cleanup_resume_height,
                restart_rewind_blocks = self.restart_rewind_blocks,
                "Deferring receipt->tx cleanup until replay catches up to the previous cursor"
            );
        }

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
    use super::{
        BlockSource, HosBlockUpdates, HosRuntimeState, StageMetrics, TransactionSource,
        VenearGovernanceModule,
    };
    use near_lake_framework::near_indexer_primitives::types::Balance;
    use near_lake_framework::near_indexer_primitives::views::{
        BlockHeaderView, BlockView, DataReceiverView, ReceiptEnumView, ReceiptView,
        SignedTransactionView,
    };
    use near_lake_framework::near_indexer_primitives::CryptoHash;
    use std::sync::Arc;

    fn crypto_hash(byte: u8) -> CryptoHash {
        CryptoHash([byte; 32])
    }

    fn near_block(height: u64) -> BlockView {
        BlockView {
            author: "author.near".parse().unwrap(),
            header: BlockHeaderView {
                height,
                prev_height: height.checked_sub(1),
                epoch_id: crypto_hash(10),
                next_epoch_id: crypto_hash(11),
                hash: crypto_hash(12),
                prev_hash: crypto_hash(13),
                prev_state_root: crypto_hash(14),
                block_body_hash: None,
                chunk_receipts_root: crypto_hash(15),
                chunk_headers_root: crypto_hash(16),
                chunk_tx_root: crypto_hash(17),
                outcome_root: crypto_hash(18),
                chunks_included: 1,
                challenges_root: crypto_hash(19),
                timestamp: 1,
                timestamp_nanosec: 1,
                random_value: crypto_hash(20),
                validator_proposals: Vec::new(),
                chunk_mask: vec![true],
                gas_price: Balance::from_yoctonear(1),
                block_ordinal: Some(height),
                rent_paid: Balance::from_yoctonear(0),
                validator_reward: Balance::from_yoctonear(0),
                total_supply: Balance::from_yoctonear(1),
                challenges_result: Vec::new(),
                last_final_block: crypto_hash(21),
                last_ds_final_block: crypto_hash(22),
                next_bp_hash: crypto_hash(23),
                block_merkle_root: crypto_hash(24),
                epoch_sync_data_hash: None,
                approvals: Vec::new(),
                signature: "ed25519:5N5CB9H1dm42Hcps225y7sY9qsKf8GugHgdGXN53BJ38qRAjPR9U1FVLtZL1NVr7Di5urN6byjHgNsZM3Rp3cr8M"
                    .parse()
                    .unwrap(),
                latest_protocol_version: 0,
                chunk_endorsements: None,
            },
            chunks: Vec::new(),
        }
    }

    fn signed_transaction(
        hash_label: &str,
        signer_id: &str,
        receiver_id: &str,
    ) -> SignedTransactionView {
        let hash_byte = hash_label
            .bytes()
            .fold(0_u8, |acc, byte| acc.wrapping_add(byte));
        SignedTransactionView {
            signer_id: signer_id.parse().unwrap(),
            public_key: "ed25519:11111111111111111111111111111111".parse().unwrap(),
            nonce: 1,
            receiver_id: receiver_id.parse().unwrap(),
            actions: Vec::new(),
            priority_fee: 0,
            signature: "ed25519:3s1BfR1CV2wAPYp8nH3UX77YPD6PEw24kTx5cHGNy8JD7BqXc9E8u6KDAdAm8YGtSNYGGyRyvE4s46HoPazTA7kG"
                .parse()
                .unwrap(),
            hash: crypto_hash(hash_byte),
        }
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
    fn hos_contract_matcher_accepts_verified_contracts_and_lockups() {
        for account_id in [
            "venear.dao",
            "vote.dao",
            "hos-root.sputnik-dao.near",
            "rewards.hos-dao.near",
            "0887fe221cafb6810909c83393e23585c439fe99.venear.dao",
        ] {
            assert!(
                VenearGovernanceModule::is_hos_contract(account_id),
                "expected {account_id} to match HoS scope"
            );
        }
    }

    #[test]
    fn hos_contract_matcher_rejects_non_hos_accounts() {
        for account_id in [
            "dao",
            "foo.vote.dao",
            "lockup.alice.venear.dao",
            "some-other.dao",
        ] {
            assert!(
                !VenearGovernanceModule::is_hos_contract(account_id),
                "expected {account_id} to be outside HoS scope"
            );
        }
    }

    #[test]
    fn recent_cache_keeps_shared_block_rows_once_per_height() {
        let mut updates = HosBlockUpdates::default();
        updates.set_block_source(BlockSource {
            author: "author.near".to_string(),
            block: Arc::new(near_block(123)),
        });
        for tx_hash in ["tx-1", "tx-2"] {
            updates.note_tx_metadata(tx_hash, false, true);
            updates.note_block_ref(tx_hash, 123);
            updates.note_pending_receipt(tx_hash, &format!("pending-{tx_hash}"));
            updates.note_transaction(
                tx_hash,
                TransactionSource {
                    block_height: 123,
                    shard_id: 0,
                    transaction: Arc::new(signed_transaction(
                        tx_hash,
                        "signer.near",
                        "receiver.near",
                    )),
                },
            );
        }

        let mut state = HosRuntimeState::default();
        let mut stats = StageMetrics::new();
        let plan = state.merge_block_updates(123, updates, false, &mut stats);

        assert!(plan.closed_hos.is_empty());
        let cache = state.recent_blocks.get(&123).expect("cache for block 123");
        assert!(cache.shared_bundle.block.is_some());
        assert_eq!(cache.shared_bundle.transactions.len(), 2);
        assert_eq!(state.graphs.len(), 2);
        assert_eq!(state.graphs["tx-1"].recent_refs.len(), 1);
        assert_eq!(state.graphs["tx-2"].recent_refs.len(), 1);
    }

    #[test]
    fn closed_non_hos_graphs_are_pruned_immediately() {
        let mut updates = HosBlockUpdates::default();
        updates.note_tx_metadata("tx-1", false, true);
        updates.note_transaction(
            "tx-1",
            TransactionSource {
                block_height: 123,
                shard_id: 0,
                transaction: Arc::new(signed_transaction("tx-1", "signer.near", "receiver.near")),
            },
        );

        let mut state = HosRuntimeState::default();
        let mut stats = StageMetrics::new();
        let plan = state.merge_block_updates(123, updates, false, &mut stats);

        assert_eq!(plan.pruned_non_hos, vec!["tx-1".to_string()]);
        assert!(state.graphs.is_empty());
    }

    #[test]
    fn data_receipt_links_survive_across_blocks() {
        let parent_receipt = action_receipt_with_data_outputs(&[9]);
        let mut updates = HosBlockUpdates::default();
        updates.note_output_data_receivers(
            &parent_receipt,
            "tx-1",
            "outcome-1",
            100,
            "block-100",
            1_000,
        );

        let mut state = HosRuntimeState::default();
        let mut stats = StageMetrics::new();
        state.merge_block_updates(100, updates, false, &mut stats);

        let link =
            VenearGovernanceModule::data_receipt_link(&state.data_receipt_links, &data_receipt(9))
                .expect("expected cached data receipt link");

        assert_eq!(link.transaction_hash, "tx-1");
        assert_eq!(
            link.parent_receipt_id,
            parent_receipt.receipt_id.to_string()
        );
        assert_eq!(link.parent_execution_outcome_id, "outcome-1");
        assert_eq!(link.block_height, 100);
    }

    #[test]
    fn receipt_tx_map_retention_clamps_to_runtime_minimum() {
        assert_eq!(
            VenearGovernanceModule::minimum_receipt_tx_map_retention_blocks(256, 100),
            556
        );
        assert_eq!(
            VenearGovernanceModule::effective_receipt_tx_map_retention_blocks_for(0, 256, 100),
            556
        );
        assert_eq!(
            VenearGovernanceModule::effective_receipt_tx_map_retention_blocks_for(1_024, 256, 100),
            1_024
        );
    }

    #[test]
    fn receipt_tx_map_cleanup_is_deferred_during_replay_catch_up() {
        let mut state = HosRuntimeState::default();
        state.set_cleanup_resume_height(Some(1_000));

        assert!(state.should_skip_receipt_tx_map_cleanup(999));
        assert!(state.should_skip_receipt_tx_map_cleanup(1_000));
        assert_eq!(state.cleanup_resume_height, Some(1_000));

        assert!(!state.should_skip_receipt_tx_map_cleanup(1_001));
        assert_eq!(state.cleanup_resume_height, None);
        assert!(!state.should_skip_receipt_tx_map_cleanup(1_002));
    }
}
