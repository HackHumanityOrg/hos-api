use anyhow::{Context, Result};
use async_trait::async_trait;
use common::config::{AppConfig, NearModuleStreamMode};
use near_lake_framework::near_indexer_primitives::views::{ReceiptEnumView, ReceiptView};
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{error, info, instrument, warn};

use crate::core::block_artifacts::BlockArtifacts;
use crate::core::block_facts::BlockFacts;
use crate::core::canonical_persistence::{
    self, NearReceiptWriteMode, NearTransactionWriteMode, CANONICAL_BLOCK_BULK_INSERT_PLAN,
    CANONICAL_CHUNK_BULK_INSERT_PLAN, CANONICAL_EXECUTION_OUTCOME_BULK_INSERT_PLAN,
    CANONICAL_RECEIPT_ACTION_BULK_INSERT_PLAN, CANONICAL_RECEIPT_BULK_INSERT_PLAN,
    CANONICAL_RECEIPT_EDGE_BULK_INSERT_PLAN, CANONICAL_STATE_CHANGE_BULK_INSERT_PLAN,
    CANONICAL_TRANSACTION_BULK_INSERT_PLAN, RECEIPT_TX_MAP_BULK_INSERT_PLAN,
};
use crate::core::canonical_rows::{
    self, ExecutionOutcomeRowMeta, NearBlockRow, NearChunkRow, NearExecutionOutcomeRow,
    NearReceiptActionRow, NearReceiptEdgeRow, NearReceiptRow, NearStateChangesRow,
    NearTransactionRow, ReceiptActionRowMeta, ReceiptTxHashMapRow,
};
use crate::core::cursor;
use crate::core::module_demand::ModuleDemand;
use crate::core::receipt_graph_resolver::{
    DataReceiptParentLink as SharedDataReceiptParentLink, ReceiptGraphResolver,
    ReceiptGraphUpsertMode,
};
use crate::core::stage_timing::{BlockProcessingLog, StageMetrics};
use crate::modules::effective_venear_data_receipt_link_retention_blocks;
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
const STALLED_PENDING_GRAPH_WARNING_BLOCKS: i64 = 43_200;
const RECEIPT_TX_MAP_CLEANUP_TARGET_ROWS: usize = 30_000;

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
    transaction_hashes: HashSet<String>,
    receipt_ids: HashSet<String>,
    execution_outcome_ids: HashSet<String>,
    receipt_edge_keys: HashSet<(String, String)>,
}

impl TxSliceRefs {
    fn merge(&mut self, other: TxSliceRefs) {
        self.transaction_hashes.extend(other.transaction_hashes);
        self.receipt_ids.extend(other.receipt_ids);
        self.execution_outcome_ids
            .extend(other.execution_outcome_ids);
        self.receipt_edge_keys.extend(other.receipt_edge_keys);
    }

    fn is_empty(&self) -> bool {
        self.transaction_hashes.is_empty()
            && self.receipt_ids.is_empty()
            && self.execution_outcome_ids.is_empty()
            && self.receipt_edge_keys.is_empty()
    }

    fn row_count(&self) -> usize {
        self.transaction_hashes.len()
            + self.receipt_ids.len()
            + self.execution_outcome_ids.len()
            + self.receipt_edge_keys.len()
    }
}

#[derive(Default)]
struct GraphBlockUpdate {
    has_hos_touch: bool,
    root_transaction_seen: bool,
    pending_receipts_add: HashSet<String>,
    pending_receipts_remove: HashSet<String>,
    yielded_receipts: HashMap<String, i64>,
    resumed_yield_data_ids: HashSet<String>,
    yield_tracking_failed: bool,
    refs: TxSliceRefs,
}

#[derive(Default)]
struct HosBlockUpdates {
    receipt_tx_hash_map: HashMap<String, ReceiptTxHashMapRow>,
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
                is_persisted_graph: false,
            });
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

    fn note_yielded_receipt(&mut self, transaction_hash: &str, data_id: &str, timeout_height: i64) {
        self.graph_update_mut(transaction_hash)
            .yielded_receipts
            .insert(data_id.to_string(), timeout_height);
    }

    fn note_resumed_yield_receipt(&mut self, transaction_hash: &str, data_id: &str) {
        self.graph_update_mut(transaction_hash)
            .resumed_yield_data_ids
            .insert(data_id.to_string());
    }

    fn note_yield_tracking_failure(&mut self, transaction_hash: &str) {
        self.graph_update_mut(transaction_hash)
            .yield_tracking_failed = true;
    }

    fn set_block_source(&mut self, row: BlockSource) {
        if self.shared_bundle.block.is_none() {
            self.shared_bundle.block = Some(row);
        }
    }

    fn remember_chunk_source(&mut self, row: ChunkSource) {
        let key = row.chunk.header.chunk_hash.to_string();
        self.shared_bundle.chunks.entry(key).or_insert(row);
    }

    fn remember_state_changes_source(&mut self, row: StateChangesSource) {
        let key = (row.block_height, row.shard_id);
        self.shared_bundle.state_changes.entry(key).or_insert(row);
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
    incomplete_reason: Option<String>,
}

impl GraphState {
    fn expire_timed_out_yields(&mut self, block_height: i64) {
        self.open_yield_receipts
            .retain(|_, timeout_height| *timeout_height > block_height);
    }

    fn resolve_yield_receipts(&mut self, resumed_data_ids: &HashSet<String>) {
        if resumed_data_ids.is_empty() || self.open_yield_receipts.is_empty() {
            return;
        }

        for data_id in resumed_data_ids {
            self.open_yield_receipts.remove(data_id);
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
    recent_block_ref_counts: HashMap<i64, usize>,
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
    retained_block_count: usize,
    stale_block_count: usize,
}

#[derive(Default)]
struct ReceiptTxMapCleanupStats {
    deleted_total: u64,
    batch_count: u64,
}

struct ProjectionResult {
    stats: StageMetrics,
    unresolved_hos_receipts: Vec<String>,
    new_data_receipt_links: HashMap<String, SharedDataReceiptParentLink>,
    receipt_map_rows: Vec<ReceiptTxHashMapRow>,
    persisted_receipt_map_rows: Vec<ReceiptTxHashMapRow>,
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

    fn apply_recent_block_ref_delta(&mut self, block_height: i64, delta: isize) {
        if delta == 0 {
            return;
        }

        let current = self
            .recent_block_ref_counts
            .get(&block_height)
            .copied()
            .unwrap_or_default();

        if delta > 0 {
            self.recent_block_ref_counts
                .insert(block_height, current.saturating_add(delta as usize));
            return;
        }

        let decrement = delta.unsigned_abs();
        if current <= decrement {
            self.recent_block_ref_counts.remove(&block_height);
        } else {
            self.recent_block_ref_counts
                .insert(block_height, current - decrement);
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
            graph_updates,
            shared_bundle,
        } = updates;
        merge_metrics.record_count("graph_update_count", graph_updates.len());
        merge_metrics.record_count("shared_bundle_row_count", shared_bundle.row_count());

        let update_loop_start = Instant::now();
        let mut recent_ref_count_deltas = Vec::new();
        let mut recent_ref_graph_count = 0_u64;
        let mut current_slice_graph_count = 0_u64;
        let mut current_slice_row_count = 0_u64;
        let mut became_hos_touch_count = 0_u64;
        for (transaction_hash, update) in graph_updates {
            let update_ref_row_count = update.refs.row_count();
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
            let had_hos_touch = graph.has_hos_touch;
            graph.has_hos_touch |= update.has_hos_touch;
            if !had_hos_touch && graph.has_hos_touch {
                became_hos_touch_count += 1;
            }
            graph.root_transaction_seen |= update.root_transaction_seen;

            for receipt_id in update.pending_receipts_add {
                graph.pending_receipts.insert(receipt_id);
            }
            for receipt_id in update.pending_receipts_remove {
                graph.pending_receipts.remove(&receipt_id);
            }
            graph.open_yield_receipts.extend(update.yielded_receipts);
            graph.resolve_yield_receipts(&update.resumed_yield_data_ids);
            graph.expire_timed_out_yields(block_height);
            if update.yield_tracking_failed && graph.incomplete_reason.is_none() {
                graph.incomplete_reason = Some("untracked_promise_yield".to_string());
            }

            if update_ref_row_count > 0 {
                current_slice_graph_count += 1;
                current_slice_row_count = current_slice_row_count
                    .saturating_add(u64::try_from(update_ref_row_count).unwrap_or(u64::MAX));
            }

            if !raw_enabled && !update.refs.is_empty() {
                recent_ref_graph_count += 1;
                let recent_refs = graph.recent_refs.entry(block_height).or_default();
                let had_recent_refs = !recent_refs.is_empty();
                recent_refs.merge(update.refs);
                if !had_recent_refs && !recent_refs.is_empty() {
                    recent_ref_count_deltas.push((block_height, 1));
                }
            }

            if !graph.root_transaction_seen && graph.has_hos_touch {
                if graph.incomplete_reason.is_none() {
                    graph.incomplete_reason = Some("missing_root_transaction_row".to_string());
                }
            } else if graph.incomplete_reason.as_deref() == Some("missing_root_transaction_row") {
                graph.incomplete_reason = None;
            }
        }
        merge_metrics.record_phase_elapsed("graph_update_loop", update_loop_start);
        merge_metrics.record_count("recent_ref_graph_count", recent_ref_graph_count);
        merge_metrics.record_count("current_slice_graph_count", current_slice_graph_count);
        merge_metrics.record_count("current_slice_row_count", current_slice_row_count);
        merge_metrics.record_count("became_hos_touch_count", became_hos_touch_count);
        for (height, delta) in recent_ref_count_deltas {
            self.apply_recent_block_ref_delta(height, delta);
        }

        if !raw_enabled && recent_ref_graph_count > 0 && shared_bundle.row_count() > 0 {
            let cache_insert_start = Instant::now();
            self.recent_blocks
                .insert(block_height, RecentBlockCache { shared_bundle });
            merge_metrics.record_phase_elapsed("recent_cache_insert", cache_insert_start);
        }

        let mut plan = BlockMergePlan::default();
        let mut remove_now = Vec::new();
        let mut closed_hos_candidates = Vec::new();
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
            closed_hos_candidates.push(transaction_hash.clone());
        }
        merge_metrics.record_phase_elapsed("closure_scan", closure_scan_start);
        merge_metrics.record_count("closed_hos_candidate_count", closed_hos_candidates.len());
        let hydrate_closed_hos_start = Instant::now();
        let mut hydrated_graph_count = 0_u64;
        let mut hydrated_row_count = 0_u64;
        for transaction_hash in closed_hos_candidates {
            let Some(graph) = self.graphs.get(&transaction_hash) else {
                continue;
            };
            let bundle = self.hydrate_bundle_from_recent_cache(graph);
            hydrated_graph_count += 1;
            hydrated_row_count = hydrated_row_count
                .saturating_add(u64::try_from(bundle.row_count()).unwrap_or(u64::MAX));

            if !graph.root_transaction_seen || !bundle.has_root_transaction(&transaction_hash) {
                plan.dropped_incomplete_hos.push((
                    transaction_hash.clone(),
                    graph
                        .incomplete_reason
                        .clone()
                        .unwrap_or_else(|| "missing_root_transaction_row".to_string()),
                ));
                remove_now.push(transaction_hash);
                continue;
            }

            if let Some(reason) = &graph.incomplete_reason {
                plan.dropped_incomplete_hos
                    .push((transaction_hash.clone(), reason.clone()));
                remove_now.push(transaction_hash);
                continue;
            }

            plan.closed_hos.push((transaction_hash.clone(), bundle));
            remove_now.push(transaction_hash);
        }
        merge_metrics.record_phase_elapsed("hydrate_closed_hos", hydrate_closed_hos_start);
        merge_metrics.record_count("hydrated_graph_count", hydrated_graph_count);
        merge_metrics.record_count("hydrated_row_count", hydrated_row_count);

        merge_metrics.record_count("remove_now_count", remove_now.len());

        for tx_hash in remove_now {
            if let Some(graph) = self.graphs.remove(&tx_hash) {
                for height in graph.recent_refs.keys().copied() {
                    self.apply_recent_block_ref_delta(height, -1);
                }
            }
        }

        if !raw_enabled {
            let cache_evict_start = Instant::now();
            let eviction_stats = self.evict_unreferenced_source_blocks();
            merge_metrics.record_phase_elapsed("recent_cache_evict", cache_evict_start);
            merge_metrics.record_count("evicted_block_count", eviction_stats.evicted_block_count);
            merge_metrics.record_count("retained_block_count", eviction_stats.retained_block_count);
            merge_metrics.record_count("stale_block_count", eviction_stats.stale_block_count);
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
        let mut shard_keys = HashSet::new();

        if !refs.is_empty() {
            if let Some(block_source) = shared_bundle.block.as_ref() {
                let block_height = block_source.block.header.height as i64;
                if let Ok(row) = VenearGovernanceModule::block_source_to_row(block_source) {
                    bundle.blocks.insert(block_height, row);
                }
            }
        }
        for hash in &refs.transaction_hashes {
            if let Some(row) = shared_bundle.transactions.get(hash) {
                shard_keys.insert((row.block_height, row.shard_id));
                if let Ok(materialized) = VenearGovernanceModule::transaction_source_to_row(row) {
                    bundle.transactions.insert(hash.clone(), materialized);
                }
            }
        }
        for receipt_id in &refs.receipt_ids {
            if let Some(row) = shared_bundle.receipts.get(receipt_id) {
                shard_keys.insert((row.block_height, row.shard_id));
                if let Ok(materialized) = VenearGovernanceModule::receipt_source_to_row(row) {
                    bundle.receipts.insert(receipt_id.clone(), materialized);
                }
            }
        }
        for outcome_id in &refs.execution_outcome_ids {
            if let Some(row) = shared_bundle.execution_outcomes.get(outcome_id) {
                shard_keys.insert((row.block_height, row.shard_id));
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
        for (chunk_hash, row) in &shared_bundle.chunks {
            if shard_keys.contains(&(row.block_height, row.shard_id)) {
                if let Ok(materialized) = VenearGovernanceModule::chunk_source_to_row(row) {
                    bundle.chunks.insert(chunk_hash.clone(), materialized);
                }
            }
        }
        for (key, row) in &shared_bundle.state_changes {
            if shard_keys.contains(key) {
                if let Ok(materialized) = VenearGovernanceModule::state_changes_source_to_row(row) {
                    bundle.state_changes.insert(*key, materialized);
                }
            }
        }

        bundle
    }

    fn evict_unreferenced_source_blocks(&mut self) -> RecentCacheEvictionStats {
        let mut stats = RecentCacheEvictionStats::default();
        stats.retained_block_count = self.recent_block_ref_counts.len();

        let stale_heights = self
            .recent_blocks
            .keys()
            .copied()
            .filter(|height| !self.recent_block_ref_counts.contains_key(height))
            .collect::<Vec<_>>();
        stats.stale_block_count = stale_heights.len();

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
    receipt_graph: Arc<ReceiptGraphResolver>,
    state: Mutex<HosRuntimeState>,
}

impl VenearGovernanceModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        Self::with_receipt_graph(
            config,
            Arc::new(
                ReceiptGraphResolver::with_data_receipt_link_retention_blocks(
                    Self::data_receipt_link_retention_blocks(config),
                ),
            ),
        )
    }

    pub fn with_receipt_graph(
        config: &common::config::AppConfig,
        receipt_graph: Arc<ReceiptGraphResolver>,
    ) -> Self {
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
            receipt_graph,
            state: Mutex::new(HosRuntimeState::default()),
        }
    }

    fn data_receipt_link_retention_blocks(config: &AppConfig) -> i64 {
        let AppConfig::Indexer(indexer) = config else {
            return 1;
        };

        let Some(near) = indexer.near.as_ref() else {
            return 1;
        };

        let Some(venear_cfg) = near
            .modules
            .as_ref()
            .and_then(|modules| modules.venear_governance.as_ref())
        else {
            return 1;
        };

        if !venear_cfg.enabled {
            return 1;
        }

        effective_venear_data_receipt_link_retention_blocks(venear_cfg)
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

    fn block_source_to_row(source: &BlockSource) -> Result<NearBlockRow> {
        canonical_rows::block_to_row(source.block.as_ref(), &source.author)
    }

    fn chunk_source_to_row(source: &ChunkSource) -> Result<NearChunkRow> {
        canonical_rows::chunk_to_row(source.chunk.as_ref(), source.block_height, source.shard_id)
    }

    fn state_changes_source_to_row(source: &StateChangesSource) -> Result<NearStateChangesRow> {
        canonical_rows::state_changes_to_row(
            source.state_changes.as_ref(),
            source.block_height,
            source.shard_id,
        )
    }

    fn transaction_source_to_row(source: &TransactionSource) -> Result<NearTransactionRow> {
        canonical_rows::transaction_to_row(
            source.transaction.as_ref(),
            source.block_height,
            source.shard_id,
        )
    }

    fn receipt_source_to_row(source: &ReceiptSource) -> Result<NearReceiptRow> {
        canonical_rows::receipt_to_row(
            source.receipt.as_ref(),
            source.block_height,
            source.shard_id,
        )
    }

    fn execution_outcome_source_to_row(
        source: &ExecutionOutcomeSource,
    ) -> Result<NearExecutionOutcomeRow> {
        canonical_rows::execution_outcome_to_row(
            source.execution_outcome.as_ref(),
            ExecutionOutcomeRowMeta {
                transaction_hash: Some(&source.transaction_hash),
                block_height: source.block_height,
                shard_id: source.shard_id,
                receipt_id: source.receipt_id.as_deref(),
                block_hash: &source.block_hash,
                chunk_hash: &source.chunk_hash,
                block_timestamp: source.block_timestamp,
            },
        )
    }

    fn receipt_action_rows_from_execution_outcome(
        source: &ExecutionOutcomeSource,
    ) -> Result<Vec<NearReceiptActionRow>> {
        let Some(receipt) = source.receipt.as_ref() else {
            return Ok(Vec::new());
        };

        canonical_rows::receipt_action_rows(
            receipt,
            ReceiptActionRowMeta {
                transaction_hash: Some(&source.transaction_hash),
                block_height: source.block_height,
                execution_outcome_id: &source.id,
                shard_id: source.shard_id,
                block_hash: &source.block_hash,
                chunk_hash: &source.chunk_hash,
                author: &source.author,
                block_timestamp: source.block_timestamp,
            },
        )
    }

    async fn flush_receipt_tx_hash_map_upserts(
        db: &impl ConnectionTrait,
        rows: &[ReceiptTxHashMapRow],
    ) -> Result<Vec<ReceiptTxHashMapRow>> {
        canonical_persistence::flush_receipt_tx_hash_map_upserts(db, rows, MODULE_NAME).await
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

    fn yield_data_ids(
        receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
    ) -> Vec<String> {
        let ReceiptEnumView::Action {
            output_data_receivers,
            ..
        } = &receipt.receipt
        else {
            return Vec::new();
        };

        output_data_receivers
            .iter()
            .map(|receiver| receiver.data_id.to_string())
            .collect()
    }

    fn receipt_data_id(
        receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
    ) -> Option<String> {
        let ReceiptEnumView::Data { data_id, .. } = &receipt.receipt else {
            return None;
        };

        Some(data_id.to_string())
    }

    fn data_receipt_link<'a>(
        links: &'a HashMap<String, DataReceiptParentLink>,
        receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
    ) -> Option<&'a DataReceiptParentLink> {
        let data_id = Self::receipt_data_id(receipt)?;
        links.get(&data_id)
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
        canonical_persistence::flush_near_transactions(db, rows, NearTransactionWriteMode::Upsert)
            .await
    }

    async fn flush_canonical_receipts(
        db: &impl ConnectionTrait,
        rows: &[NearReceiptRow],
    ) -> Result<()> {
        canonical_persistence::flush_near_receipts(db, rows, NearReceiptWriteMode::Upsert).await
    }

    async fn flush_canonical_execution_outcomes(
        db: &impl ConnectionTrait,
        rows: &[NearExecutionOutcomeRow],
    ) -> Result<()> {
        canonical_persistence::flush_near_execution_outcomes(db, rows).await
    }

    async fn flush_canonical_receipt_actions(
        db: &impl ConnectionTrait,
        rows: &[NearReceiptActionRow],
    ) -> Result<()> {
        canonical_persistence::flush_near_receipt_actions(db, rows).await
    }

    async fn flush_canonical_receipt_edges(
        db: &impl ConnectionTrait,
        rows: &[NearReceiptEdgeRow],
    ) -> Result<()> {
        canonical_persistence::flush_near_receipt_edges(db, rows).await
    }

    async fn flush_canonical_blocks(
        db: &impl ConnectionTrait,
        rows: &[NearBlockRow],
    ) -> Result<()> {
        canonical_persistence::flush_near_blocks(db, rows).await
    }

    async fn flush_canonical_chunks(
        db: &impl ConnectionTrait,
        rows: &[NearChunkRow],
    ) -> Result<()> {
        canonical_persistence::flush_near_chunks(db, rows).await
    }

    async fn flush_canonical_state_changes(
        db: &impl ConnectionTrait,
        rows: &[NearStateChangesRow],
    ) -> Result<()> {
        canonical_persistence::flush_near_state_changes(db, rows).await
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

        stats.record_count("canonical_transaction_rows", transactions.len());
        stats.record_count("canonical_receipt_rows", receipts.len());
        stats.record_count("canonical_execution_outcome_rows", execution_outcomes.len());
        stats.record_count("canonical_receipt_action_rows", receipt_actions.len());
        stats.record_count("canonical_receipt_edge_rows", receipt_edges.len());
        stats.record_count("canonical_block_rows", blocks.len());
        stats.record_count("canonical_chunk_rows", chunks.len());
        stats.record_count("canonical_state_changes_rows", state_changes.len());
        stats.record_count(
            "canonical_total_batches",
            canonical_persistence::batch_count(
                transactions.len(),
                CANONICAL_TRANSACTION_BULK_INSERT_PLAN,
            ) + canonical_persistence::batch_count(
                receipts.len(),
                CANONICAL_RECEIPT_BULK_INSERT_PLAN,
            ) + canonical_persistence::batch_count(
                execution_outcomes.len(),
                CANONICAL_EXECUTION_OUTCOME_BULK_INSERT_PLAN,
            ) + canonical_persistence::batch_count(
                receipt_actions.len(),
                CANONICAL_RECEIPT_ACTION_BULK_INSERT_PLAN,
            ) + canonical_persistence::batch_count(
                receipt_edges.len(),
                CANONICAL_RECEIPT_EDGE_BULK_INSERT_PLAN,
            ) + canonical_persistence::batch_count(blocks.len(), CANONICAL_BLOCK_BULK_INSERT_PLAN)
                + canonical_persistence::batch_count(
                    chunks.len(),
                    CANONICAL_CHUNK_BULK_INSERT_PLAN,
                )
                + canonical_persistence::batch_count(
                    state_changes.len(),
                    CANONICAL_STATE_CHANGE_BULK_INSERT_PLAN,
                ),
        );

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
    ) -> Result<ReceiptTxMapCleanupStats> {
        let cutoff_height =
            block_height.saturating_sub(self.effective_receipt_tx_map_retention_blocks());
        let mut values = vec![cutoff_height.into()];
        let mut sql = String::from(
            "WITH deletable AS (
                SELECT m.receipt_id
                FROM near.near_receipt_tx_hash_map m
                WHERE m.is_persisted_graph = FALSE
                  AND m.last_seen_height < $1",
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

        let limit_placeholder = values.len() + 1;
        values.push((RECEIPT_TX_MAP_CLEANUP_TARGET_ROWS as i64).into());
        sql.push_str(&format!(
            "
                ORDER BY m.last_seen_height ASC
                LIMIT ${limit_placeholder}
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

        Ok(ReceiptTxMapCleanupStats {
            deleted_total: result.rows_affected(),
            batch_count: 1,
        })
    }

    async fn store_hos_projection(
        &self,
        facts: &BlockFacts,
        msg: &StreamerMessage,
        db: &impl ConnectionTrait,
    ) -> Result<ProjectionResult> {
        let mut stats = StageMetrics::new();
        let block_height = facts.block_height;
        let block_hash = facts.block_hash.clone();
        let block_timestamp = facts.block_timestamp;
        let default_author = facts.block_author.clone();
        let block_source = BlockSource {
            author: default_author.clone(),
            block: Arc::new(msg.block.clone()),
        };

        let resolved_graph_start = Instant::now();
        let resolved_graph = self.receipt_graph.resolve_for_block(msg, facts, db).await?;
        stats.record_phase_elapsed("receipt_graph_resolve", resolved_graph_start);
        stats.merge_with_prefix("receipt_graph.", resolved_graph.metrics.clone());

        let mut receipt_tx_hashes = resolved_graph.receipt_tx_hashes.clone();
        let lookup_receipt_ids = facts.candidate_receipt_ids.clone();
        let unresolved_lookup_receipt_ids = resolved_graph.unresolved_receipt_ids.clone();
        let lookup_stats = resolved_graph.lookup_stats.clone();
        let relevant_data_receipt_ids = facts.data_receipt_ids.clone();
        let mut data_receipt_links = resolved_graph
            .data_receipt_links
            .into_iter()
            .filter_map(|(data_id, link)| {
                link.transaction_hash.map(|transaction_hash| {
                    (
                        data_id,
                        DataReceiptParentLink {
                            transaction_hash,
                            parent_receipt_id: link.parent_receipt_id,
                            parent_execution_outcome_id: link.parent_execution_outcome_id,
                            block_height: link.block_height,
                            block_hash: link.block_hash,
                            block_timestamp: link.block_timestamp,
                        },
                    )
                })
            })
            .collect::<HashMap<_, _>>();
        let mut new_data_receipt_links = HashMap::new();
        stats.record_count(
            "lookup_receipt_ids_before_dedup",
            facts.candidate_receipt_id_count_before_dedup,
        );
        stats.record_count(
            "prepared_receipt_tx_seed_count",
            facts.receipt_tx_hashes.len(),
        );
        stats.record_count(
            "prepared_data_receipt_id_count",
            facts.data_receipt_ids.len(),
        );
        stats.record_count(
            "prepared_candidate_receipt_id_count",
            facts.candidate_receipt_ids.len(),
        );
        stats.record_count("lookup_receipt_ids_after_dedup", lookup_receipt_ids.len());
        stats.record_count(
            "data_receipt_link_cache_size",
            resolved_graph.data_receipt_link_cache_size,
        );
        stats.record_count(
            "data_receipt_link_lookup_id_count",
            relevant_data_receipt_ids.len(),
        );
        stats.record_count("data_receipt_link_snapshot_size", data_receipt_links.len());
        stats.record_count("receipt_lookup_count", lookup_receipt_ids.len());
        stats.record_count(
            "receipt_lookup_candidate_count",
            lookup_stats.candidate_count,
        );
        stats.record_count(
            "receipt_lookup_local_resolved_count",
            lookup_stats.local_resolved_count,
        );
        stats.record_count(
            "receipt_lookup_hot_cache_hit_count",
            lookup_stats.hot_cache_hit_count,
        );
        stats.record_count(
            "receipt_lookup_db_receipt_count",
            unresolved_lookup_receipt_ids.len(),
        );
        stats.record_count("receipt_lookup_db_query_count", lookup_stats.db_query_count);
        stats.record_count("receipt_lookup_db_hit_count", lookup_stats.db_hit_count);
        stats.record_count("receipt_lookup_db_miss_count", lookup_stats.db_miss_count);

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
                    Self::cache_output_data_receivers(
                        &mut data_receipt_links,
                        receipt,
                        &tx_hash,
                        &tx_outcome_id,
                        block_height,
                        &block_hash,
                        block_timestamp,
                    );
                    Self::cache_output_data_receivers(
                        &mut new_data_receipt_links,
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
                    transaction_hash = data_receipt_link.map(|link| link.transaction_hash.clone());
                }
                let Some(transaction_hash) = transaction_hash else {
                    continue;
                };
                let outcome_id = outcome_with_receipt.execution_outcome.id.to_string();

                Self::cache_output_data_receivers(
                    &mut data_receipt_links,
                    receipt,
                    &transaction_hash,
                    &outcome_id,
                    block_height,
                    &block_hash,
                    block_timestamp,
                );
                Self::cache_output_data_receivers(
                    &mut new_data_receipt_links,
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
        for (shard, shard_ctx) in msg.shards.iter().zip(facts.shards.iter()) {
            let mut shard_touched_tx_hashes = HashSet::new();
            let chunk_hash = shard_ctx.chunk_hash.clone();
            let chunk_author = shard_ctx.chunk_author.clone();
            let shard_id = shard_ctx.shard_id;

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
                            let yielded_data_ids = Self::yield_data_ids(receipt);
                            if yielded_data_ids.is_empty() {
                                updates.note_yield_tracking_failure(&tx_hash);
                            } else {
                                for data_id in yielded_data_ids {
                                    updates.note_yielded_receipt(
                                        &tx_hash,
                                        &data_id,
                                        block_height.saturating_add(DEFAULT_YIELD_TIMEOUT_BLOCKS),
                                    );
                                }
                            }
                        }
                        Self::cache_output_data_receivers(
                            &mut data_receipt_links,
                            receipt,
                            &tx_hash,
                            &tx_outcome_id,
                            block_height,
                            &block_hash,
                            block_timestamp,
                        );
                        Self::cache_output_data_receivers(
                            &mut new_data_receipt_links,
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
                                    transaction_hash: Some(tx_hash.clone()),
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
                        transaction_hash =
                            data_receipt_link.map(|link| link.transaction_hash.clone());
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
                    if let Some(link) = data_receipt_link {
                        updates.note_receipt_edge(
                            &transaction_hash,
                            NearReceiptEdgeRow {
                                parent_receipt_id: link.parent_receipt_id.clone(),
                                child_receipt_id: receipt_id.clone(),
                                transaction_hash: Some(transaction_hash.clone()),
                                parent_execution_outcome_id: link
                                    .parent_execution_outcome_id
                                    .clone(),
                                block_height: link.block_height,
                                block_hash: link.block_hash.clone(),
                                block_timestamp: link.block_timestamp,
                            },
                        );
                    }
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
                        let yielded_data_ids = Self::yield_data_ids(receipt);
                        if yielded_data_ids.is_empty() {
                            updates.note_yield_tracking_failure(&transaction_hash);
                        } else {
                            for data_id in yielded_data_ids {
                                updates.note_yielded_receipt(
                                    &transaction_hash,
                                    &data_id,
                                    block_height.saturating_add(DEFAULT_YIELD_TIMEOUT_BLOCKS),
                                );
                            }
                        }
                    }
                    if Self::is_promise_resume(&receipt.receipt) {
                        if let Some(data_id) = Self::receipt_data_id(receipt) {
                            updates.note_resumed_yield_receipt(&transaction_hash, &data_id);
                        }
                    }
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                let receipt_id = receipt.receipt_id.to_string();
                let data_receipt_link = Self::data_receipt_link(&data_receipt_links, receipt);
                let mut transaction_hash = receipt_tx_hashes.get(&receipt_id).cloned();
                if transaction_hash.is_none() {
                    transaction_hash = data_receipt_link.map(|link| link.transaction_hash.clone());
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
                if let Some(link) = data_receipt_link {
                    updates.note_receipt_edge(
                        &transaction_hash,
                        NearReceiptEdgeRow {
                            parent_receipt_id: link.parent_receipt_id.clone(),
                            child_receipt_id: receipt_id.clone(),
                            transaction_hash: Some(transaction_hash.clone()),
                            parent_execution_outcome_id: link.parent_execution_outcome_id.clone(),
                            block_height: link.block_height,
                            block_hash: link.block_hash.clone(),
                            block_timestamp: link.block_timestamp,
                        },
                    );
                }

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
                    let yielded_data_ids = Self::yield_data_ids(receipt);
                    if yielded_data_ids.is_empty() {
                        updates.note_yield_tracking_failure(&transaction_hash);
                    } else {
                        for data_id in yielded_data_ids {
                            updates.note_yielded_receipt(
                                &transaction_hash,
                                &data_id,
                                block_height.saturating_add(DEFAULT_YIELD_TIMEOUT_BLOCKS),
                            );
                        }
                    }
                }
                if Self::is_promise_resume(&receipt.receipt) {
                    if let Some(data_id) = Self::receipt_data_id(receipt) {
                        updates.note_resumed_yield_receipt(&transaction_hash, &data_id);
                    }
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
                            transaction_hash: Some(transaction_hash.clone()),
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
                if !shard_touched_tx_hashes.is_empty() {
                    let chunk_source = ChunkSource {
                        block_height,
                        shard_id,
                        chunk: Arc::new(chunk.clone()),
                    };
                    updates.remember_chunk_source(chunk_source);
                }
            }

            if !shard.state_changes.is_empty() && !shard_touched_tx_hashes.is_empty() {
                let state_changes_source = StateChangesSource {
                    block_height,
                    shard_id,
                    state_changes: Arc::new(shard.state_changes.clone()),
                };
                updates.remember_state_changes_source(state_changes_source);
            }
        }
        stats.record_phase_elapsed("block_update_build", block_update_build_start);

        stats.record_count("touched_tx_count", touched_tx_hashes.len());
        stats.record_count(
            "unresolved_hos_receipt_count",
            unresolved_hos_receipts.len(),
        );

        let mut receipt_map_rows = updates
            .receipt_tx_hash_map
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let (receipt_map_rows_to_write, receipt_tx_write_stats) = self.receipt_graph.plan_upserts(
            &receipt_map_rows,
            block_height,
            ReceiptGraphUpsertMode::Venear,
        );
        stats.record_count("receipt_tx_map_candidate_rows", receipt_map_rows.len());
        stats.record_count("receipt_tx_map_write_rows", receipt_map_rows_to_write.len());
        stats.record_count(
            "receipt_tx_map_write_batch_count",
            canonical_persistence::batch_count(
                receipt_map_rows_to_write.len(),
                RECEIPT_TX_MAP_BULK_INSERT_PLAN,
            ),
        );
        stats.record_count(
            "receipt_tx_map_write_new_count",
            receipt_tx_write_stats.new_count,
        );
        stats.record_count(
            "receipt_tx_map_write_refresh_count",
            receipt_tx_write_stats.refresh_count,
        );
        stats.record_count(
            "receipt_tx_map_write_skipped_count",
            receipt_tx_write_stats.skipped_count,
        );
        let receipt_map_write_start = Instant::now();
        let mut persisted_receipt_map_rows =
            Self::flush_receipt_tx_hash_map_upserts(db, &receipt_map_rows_to_write).await?;
        stats.record_phase_elapsed("receipt_tx_map_write", receipt_map_write_start);
        stats.record_count(
            "receipt_tx_hot_cache_size",
            self.receipt_graph.hot_cache_size(),
        );

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

        let mut durable_receipt_map_rows = HashMap::new();
        if !self.raw_enabled && !merge_plan.closed_hos.is_empty() {
            let mut canonical_batch = CanonicalWriteBatch::default();
            for (transaction_hash, bundle) in &merge_plan.closed_hos {
                canonical_batch.merge_bundle(bundle.clone());
                for receipt_id in bundle.receipts.keys() {
                    durable_receipt_map_rows
                        .entry(receipt_id.clone())
                        .or_insert_with(|| ReceiptTxHashMapRow {
                            receipt_id: receipt_id.clone(),
                            transaction_hash: transaction_hash.clone(),
                            block_height,
                            is_persisted_graph: true,
                        });
                }
            }

            let canonical_close_write_start = Instant::now();
            Self::flush_canonical_write_batch(db, canonical_batch, &mut stats).await?;
            stats.record_phase_elapsed("canonical_close_write", canonical_close_write_start);

            let durable_receipt_map_rows =
                durable_receipt_map_rows.into_values().collect::<Vec<_>>();
            stats.record_count(
                "receipt_tx_map_persisted_graph_rows",
                durable_receipt_map_rows.len(),
            );
            if !durable_receipt_map_rows.is_empty() {
                let durable_receipt_map_write_start = Instant::now();
                let persisted_durable_receipt_map_rows =
                    Self::flush_receipt_tx_hash_map_upserts(db, &durable_receipt_map_rows).await?;
                stats.record_phase_elapsed(
                    "receipt_tx_map_persisted_graph_write",
                    durable_receipt_map_write_start,
                );
                receipt_map_rows.extend(durable_receipt_map_rows);
                persisted_receipt_map_rows.extend(persisted_durable_receipt_map_rows);
            }

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

        Ok(ProjectionResult {
            stats,
            unresolved_hos_receipts,
            new_data_receipt_links: new_data_receipt_links
                .into_iter()
                .map(|(data_id, link)| {
                    (
                        data_id,
                        SharedDataReceiptParentLink {
                            transaction_hash: Some(link.transaction_hash),
                            parent_receipt_id: link.parent_receipt_id,
                            parent_execution_outcome_id: link.parent_execution_outcome_id,
                            block_height: link.block_height,
                            block_hash: link.block_hash,
                            block_timestamp: link.block_timestamp,
                        },
                    )
                })
                .collect(),
            receipt_map_rows,
            persisted_receipt_map_rows,
        })
    }

    async fn process_block_internal(
        &self,
        artifacts: &BlockArtifacts,
        msg: &StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()> {
        let facts = artifacts.facts();
        let height = facts.block_height as u64;
        let shard_count = facts.shard_count;
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

        let projection_result = process_log
            .metrics_mut()
            .time_future(
                "store_projection",
                self.store_hos_projection(facts, msg, &txn),
            )
            .await
            .context("failed to store HOS governance raw projection data")?;
        let ProjectionResult {
            stats: projection_stats,
            unresolved_hos_receipts,
            new_data_receipt_links,
            receipt_map_rows,
            persisted_receipt_map_rows,
        } = projection_result;
        process_log
            .metrics_mut()
            .merge_with_prefix("projection.", projection_stats);

        process_log
            .metrics_mut()
            .time_future("txn_commit", txn.commit())
            .await?;

        self.receipt_graph.remember_after_commit(
            facts.block_height,
            new_data_receipt_links,
            &receipt_map_rows,
            &persisted_receipt_map_rows,
        );
        process_log.metrics_mut().record_count(
            "receipt_tx_hot_cache_size",
            self.receipt_graph.hot_cache_size(),
        );

        let post_commit_start = Instant::now();
        let block_height = height as i64;
        let maintenance_collect_start = Instant::now();
        let maintenance_plan = {
            let state = self.state.lock().unwrap();
            state.collect_maintenance_plan(block_height, self.finalization_interval_blocks)
        };
        process_log
            .metrics_mut()
            .record_phase_elapsed("maintenance_collect", maintenance_collect_start);

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

        process_log
            .metrics_mut()
            .record_flag("maintenance_triggered", maintenance_plan.triggered);
        process_log.metrics_mut().record_count(
            "stalled_graph_count",
            maintenance_plan.stalled_warning_summary.total_count,
        );
        process_log.metrics_mut().record_count(
            "stalled_hos_graph_count",
            maintenance_plan.stalled_warning_summary.hos_touch_count,
        );
        process_log.metrics_mut().record_count(
            "effective_receipt_tx_map_retention_blocks",
            self.effective_receipt_tx_map_retention_blocks().max(0) as u64,
        );
        process_log.metrics_mut().record_count(
            "maintenance_active_graph_tx_hash_count",
            maintenance_plan.active_graph_tx_hashes.len(),
        );
        process_log
            .metrics_mut()
            .record_flag("receipt_tx_map_cleanup_skipped_for_replay", false);

        if maintenance_plan.triggered {
            let finalize_start = Instant::now();
            let cleanup_start = Instant::now();
            let cleanup_skipped_for_replay = {
                let mut state = self.state.lock().unwrap();
                state.should_skip_receipt_tx_map_cleanup(block_height)
            };
            process_log.metrics_mut().record_flag(
                "receipt_tx_map_cleanup_skipped_for_replay",
                cleanup_skipped_for_replay,
            );
            let cleanup_stats = if cleanup_skipped_for_replay {
                ReceiptTxMapCleanupStats::default()
            } else {
                self.run_receipt_tx_map_cleanup(
                    db,
                    block_height,
                    &maintenance_plan.active_graph_tx_hashes,
                )
                .await?
            };
            process_log
                .metrics_mut()
                .record_phase_elapsed("receipt_tx_map_cleanup", cleanup_start);
            process_log.metrics_mut().record_count(
                "receipt_tx_map_cleanup_batch_count",
                cleanup_stats.batch_count,
            );
            process_log
                .metrics_mut()
                .record_count("receipt_tx_map_deleted", cleanup_stats.deleted_total);
            let hot_cache_evict_start = Instant::now();
            self.receipt_graph.evict_receipt_tx_hot_cache(
                block_height,
                self.effective_receipt_tx_map_retention_blocks(),
                &maintenance_plan.active_graph_tx_hashes,
            );
            process_log.metrics_mut().record_count(
                "receipt_tx_hot_cache_size",
                self.receipt_graph.hot_cache_size(),
            );
            process_log
                .metrics_mut()
                .record_phase_elapsed("receipt_tx_hot_cache_evict", hot_cache_evict_start);
            process_log
                .metrics_mut()
                .record_phase_elapsed("maintenance_finalize", finalize_start);
        }

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

        for receipt_id in unresolved_hos_receipts {
            warn!(
                module = MODULE_NAME,
                receipt_id = %receipt_id,
                height = block_height,
                "HoS-related receipt observed without resolvable transaction hash; preserving nothing until replay or later lineage resolution"
            );
        }
        process_log
            .metrics_mut()
            .record_phase_elapsed("post_commit", post_commit_start);

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

    fn demand(&self) -> ModuleDemand {
        ModuleDemand {
            needs_receipt_graph: true,
            needs_raw_projection: false,
            needs_hos_delta: true,
        }
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

    #[instrument(skip(self, artifacts, msg, db), fields(module = "venear_governance", height = artifacts.facts().block_height))]
    async fn process_block_with_artifacts(
        &self,
        artifacts: &BlockArtifacts,
        msg: &StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()> {
        self.process_block_internal(artifacts, msg, db).await
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

    fn yield_receipt(data_id: u8) -> ReceiptView {
        let mut receipt = action_receipt_with_data_outputs(&[data_id]);
        let ReceiptEnumView::Action {
            is_promise_yield, ..
        } = &mut receipt.receipt
        else {
            unreachable!();
        };
        *is_promise_yield = true;
        receipt
    }

    fn resumed_data_receipt(data_id: u8) -> ReceiptView {
        let mut receipt = data_receipt(data_id);
        let ReceiptEnumView::Data {
            is_promise_resume, ..
        } = &mut receipt.receipt
        else {
            unreachable!();
        };
        *is_promise_resume = true;
        receipt
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
        assert_eq!(state.recent_block_ref_counts.get(&123), Some(&2));
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
    fn yield_resumes_by_data_id_not_by_count() {
        let first_yield = yield_receipt(9);
        let second_yield = yield_receipt(10);
        let resumed = resumed_data_receipt(10);
        let mut updates = HosBlockUpdates::default();
        updates.note_tx_metadata("tx-1", true, true);
        updates.note_yielded_receipt(
            "tx-1",
            &VenearGovernanceModule::yield_data_ids(&first_yield)[0],
            300,
        );
        updates.note_yielded_receipt(
            "tx-1",
            &VenearGovernanceModule::yield_data_ids(&second_yield)[0],
            300,
        );
        updates.note_resumed_yield_receipt(
            "tx-1",
            &VenearGovernanceModule::receipt_data_id(&resumed).expect("resume data id"),
        );

        let mut state = HosRuntimeState::default();
        let mut stats = StageMetrics::new();
        state.merge_block_updates(100, updates, false, &mut stats);

        assert_eq!(
            state
                .graphs
                .get("tx-1")
                .expect("graph")
                .open_yield_receipts
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            vec![VenearGovernanceModule::yield_data_ids(&first_yield)[0].clone()]
        );
    }

    #[test]
    fn duplicate_resume_observation_is_idempotent() {
        let yielded = yield_receipt(9);
        let resumed = resumed_data_receipt(9);
        let data_id = VenearGovernanceModule::yield_data_ids(&yielded)[0].clone();
        let mut updates = HosBlockUpdates::default();
        updates.note_tx_metadata("tx-1", true, true);
        updates.note_pending_receipt("tx-1", "pending-1");
        updates.note_yielded_receipt("tx-1", &data_id, 300);
        updates.note_resumed_yield_receipt(
            "tx-1",
            &VenearGovernanceModule::receipt_data_id(&resumed).expect("resume data id"),
        );
        updates.note_resumed_yield_receipt(
            "tx-1",
            &VenearGovernanceModule::receipt_data_id(&resumed).expect("resume data id"),
        );

        let mut state = HosRuntimeState::default();
        state.merge_block_updates(100, updates, false, &mut StageMetrics::new());

        assert!(state
            .graphs
            .get("tx-1")
            .expect("graph")
            .open_yield_receipts
            .is_empty());
    }

    #[test]
    fn yield_tracking_failure_marks_graph_incomplete() {
        let mut updates = HosBlockUpdates::default();
        updates.note_tx_metadata("tx-1", true, true);
        updates.note_pending_receipt("tx-1", "pending-1");
        updates.note_yield_tracking_failure("tx-1");

        let mut state = HosRuntimeState::default();
        state.merge_block_updates(100, updates, false, &mut StageMetrics::new());

        assert_eq!(
            state
                .graphs
                .get("tx-1")
                .expect("graph")
                .incomplete_reason
                .as_deref(),
            Some("untracked_promise_yield")
        );
    }

    #[test]
    fn recent_cache_ref_counts_are_released_when_graph_is_removed() {
        let mut updates = HosBlockUpdates::default();
        updates.note_tx_metadata("tx-1", false, true);
        updates.note_pending_receipt("tx-1", "pending-1");
        updates.note_transaction(
            "tx-1",
            TransactionSource {
                block_height: 123,
                shard_id: 0,
                transaction: Arc::new(signed_transaction("tx-1", "signer.near", "receiver.near")),
            },
        );

        let mut state = HosRuntimeState::default();
        state.merge_block_updates(123, updates, false, &mut StageMetrics::new());
        assert_eq!(state.recent_block_ref_counts.get(&123), Some(&1));

        let mut close_updates = HosBlockUpdates::default();
        close_updates.note_tx_metadata("tx-1", false, false);
        close_updates.note_executed_receipt("tx-1", "pending-1");

        let plan = state.merge_block_updates(124, close_updates, false, &mut StageMetrics::new());

        assert_eq!(plan.pruned_non_hos, vec!["tx-1".to_string()]);
        assert!(state.recent_block_ref_counts.is_empty());
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
