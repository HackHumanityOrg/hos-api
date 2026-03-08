use anyhow::Result;
use near_lake_framework::near_indexer_primitives::views::{ReceiptEnumView, ReceiptView};
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::ConnectionTrait;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Mutex;
use std::time::Instant;
use tracing::error;

use crate::core::block_facts::BlockFacts;
use crate::core::canonical_rows::ReceiptTxHashMapRow;
use crate::core::receipt_tx_canonical_resolver;
use crate::core::receipt_tx_hash_map::{
    load_receipt_tx_hash_map, receipt_tx_hash_lookup_batch_count,
};
use crate::core::stage_timing::StageMetrics;

const DATA_RECEIPT_LINK_RETENTION_BLOCKS: i64 = 256;
const RECEIPT_TX_MAP_REFRESH_INTERVAL_BLOCKS: i64 = 128;

#[derive(Clone)]
pub struct DataReceiptParentLink {
    pub transaction_hash: Option<String>,
    pub parent_receipt_id: String,
    pub parent_execution_outcome_id: String,
    pub block_height: i64,
    pub block_hash: String,
    pub block_timestamp: i64,
}

#[derive(Clone)]
struct ReceiptTxHotCacheEntry {
    transaction_hash: String,
    last_seen_height: i64,
    last_persisted_height: i64,
}

#[derive(Default, Clone)]
pub struct ReceiptGraphLookupStats {
    pub candidate_count: usize,
    pub local_resolved_count: usize,
    pub hot_cache_hit_count: usize,
    pub db_query_count: usize,
    pub db_hit_count: usize,
    pub db_miss_count: usize,
}

#[derive(Default)]
pub struct ReceiptGraphWriteStats {
    pub new_count: usize,
    pub refresh_count: usize,
    pub skipped_count: usize,
}

#[derive(Clone)]
pub struct ResolvedReceiptGraph {
    pub receipt_tx_hashes: HashMap<String, String>,
    pub data_receipt_links: HashMap<String, DataReceiptParentLink>,
    pub unresolved_receipt_ids: Vec<String>,
    pub data_receipt_link_cache_size: usize,
    pub metrics: StageMetrics,
    pub lookup_stats: ReceiptGraphLookupStats,
}

pub enum ReceiptGraphUpsertMode {
    Raw,
    Venear,
}

#[derive(Default)]
struct ReceiptGraphRuntimeState {
    data_receipt_links: HashMap<String, DataReceiptParentLink>,
    data_receipt_link_heights: BTreeMap<i64, Vec<String>>,
    receipt_tx_hot_cache: HashMap<String, ReceiptTxHotCacheEntry>,
}

impl ReceiptGraphRuntimeState {
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

    fn snapshot_data_receipt_links(
        &self,
        data_ids: &HashSet<String>,
    ) -> HashMap<String, DataReceiptParentLink> {
        if data_ids.is_empty() {
            return HashMap::new();
        }

        let mut snapshot =
            HashMap::with_capacity(data_ids.len().min(self.data_receipt_links.len()));
        for data_id in data_ids {
            if let Some(link) = self.data_receipt_links.get(data_id) {
                snapshot.insert(data_id.clone(), link.clone());
            }
        }
        snapshot
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

    fn resolve_receipt_tx_hashes_from_hot_cache(
        &mut self,
        block_height: i64,
        receipt_ids: &[String],
        receipt_tx_hashes: &mut HashMap<String, String>,
    ) -> ReceiptGraphLookupStats {
        let mut stats = ReceiptGraphLookupStats {
            candidate_count: receipt_ids.len(),
            ..ReceiptGraphLookupStats::default()
        };

        for receipt_id in receipt_ids {
            if receipt_tx_hashes.contains_key(receipt_id) {
                stats.local_resolved_count += 1;
                continue;
            }

            if let Some(entry) = self.receipt_tx_hot_cache.get_mut(receipt_id) {
                entry.last_seen_height = entry.last_seen_height.max(block_height);
                receipt_tx_hashes
                    .entry(receipt_id.clone())
                    .or_insert_with(|| entry.transaction_hash.clone());
                stats.hot_cache_hit_count += 1;
            }
        }

        stats
    }

    fn plan_receipt_tx_hash_map_writes(
        &self,
        rows: &[ReceiptTxHashMapRow],
        block_height: i64,
        mode: ReceiptGraphUpsertMode,
    ) -> (Vec<ReceiptTxHashMapRow>, ReceiptGraphWriteStats) {
        if matches!(mode, ReceiptGraphUpsertMode::Raw) {
            return (
                rows.to_vec(),
                ReceiptGraphWriteStats {
                    new_count: rows.len(),
                    ..ReceiptGraphWriteStats::default()
                },
            );
        }

        let mut planned = Vec::new();
        let mut stats = ReceiptGraphWriteStats::default();

        for row in rows {
            let Some(entry) = self.receipt_tx_hot_cache.get(&row.receipt_id) else {
                planned.push(row.clone());
                stats.new_count += 1;
                continue;
            };

            if entry.transaction_hash != row.transaction_hash {
                planned.push(row.clone());
                stats.refresh_count += 1;
                continue;
            }

            if block_height.saturating_sub(entry.last_persisted_height)
                >= RECEIPT_TX_MAP_REFRESH_INTERVAL_BLOCKS
            {
                planned.push(row.clone());
                stats.refresh_count += 1;
            } else {
                stats.skipped_count += 1;
            }
        }

        (planned, stats)
    }

    fn remember_receipt_tx_hashes(
        &mut self,
        block_height: i64,
        rows: &[ReceiptTxHashMapRow],
        persisted_rows: &[ReceiptTxHashMapRow],
    ) {
        let mut persisted_by_receipt = HashMap::new();
        for row in persisted_rows {
            persisted_by_receipt.insert(row.receipt_id.as_str(), row.transaction_hash.as_str());
        }

        for row in rows {
            let persisted_tx_hash = persisted_by_receipt
                .get(row.receipt_id.as_str())
                .copied()
                .unwrap_or(row.transaction_hash.as_str());
            let entry = self
                .receipt_tx_hot_cache
                .entry(row.receipt_id.clone())
                .or_insert_with(|| ReceiptTxHotCacheEntry {
                    transaction_hash: persisted_tx_hash.to_string(),
                    last_seen_height: block_height,
                    last_persisted_height: block_height,
                });
            entry.transaction_hash = persisted_tx_hash.to_string();
            entry.last_seen_height = entry.last_seen_height.max(block_height);
            if persisted_by_receipt.contains_key(row.receipt_id.as_str()) {
                entry.last_persisted_height = block_height;
            }
        }
    }

    fn evict_receipt_tx_hot_cache(
        &mut self,
        block_height: i64,
        retention_blocks: i64,
        active_graph_tx_hashes: &HashSet<String>,
    ) {
        let cutoff_height = block_height.saturating_sub(retention_blocks);
        self.receipt_tx_hot_cache.retain(|_, entry| {
            entry.last_seen_height >= cutoff_height
                || active_graph_tx_hashes.contains(&entry.transaction_hash)
        });
    }
}

#[derive(Default)]
pub struct ReceiptGraphResolver {
    state: Mutex<ReceiptGraphRuntimeState>,
}

impl ReceiptGraphResolver {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn resolve_for_block(
        &self,
        msg: &StreamerMessage,
        facts: &BlockFacts,
        db: &impl ConnectionTrait,
    ) -> Result<ResolvedReceiptGraph> {
        let mut metrics = StageMetrics::new();
        let clone_seed_start = Instant::now();
        let mut receipt_tx_hashes = facts.receipt_tx_hashes.clone();
        metrics.record_phase_elapsed("seed_clone", clone_seed_start);

        let snapshot_start = Instant::now();
        let (data_receipt_links, data_receipt_link_cache_size) = {
            let state = self.state.lock().unwrap();
            (
                state.snapshot_data_receipt_links(&facts.data_receipt_ids),
                state.data_receipt_links.len(),
            )
        };
        metrics.record_phase_elapsed("data_link_snapshot", snapshot_start);
        metrics.record_count("data_receipt_link_cache_size", data_receipt_link_cache_size);
        metrics.record_count(
            "data_receipt_link_snapshot_size",
            data_receipt_links.len(),
        );

        let seed_from_links_start = Instant::now();
        Self::seed_receipt_tx_hashes_from_data_links(
            msg,
            &data_receipt_links,
            &mut receipt_tx_hashes,
        );
        metrics.record_phase_elapsed("seed_from_data_links", seed_from_links_start);

        let hot_cache_start = Instant::now();
        let mut lookup_stats = {
            let mut state = self.state.lock().unwrap();
            state.resolve_receipt_tx_hashes_from_hot_cache(
                facts.block_height,
                &facts.candidate_receipt_ids,
                &mut receipt_tx_hashes,
            )
        };
        metrics.record_phase_elapsed("hot_cache_resolve", hot_cache_start);

        let unresolved_receipt_ids = facts
            .candidate_receipt_ids
            .iter()
            .filter(|receipt_id| !receipt_tx_hashes.contains_key(*receipt_id))
            .cloned()
            .collect::<Vec<_>>();
        lookup_stats.db_query_count =
            receipt_tx_hash_lookup_batch_count(unresolved_receipt_ids.len());

        let db_lookup_start = Instant::now();
        let mapped_receipts = load_receipt_tx_hash_map(db, &unresolved_receipt_ids).await?;
        let mapped_receipt_rows = mapped_receipts
            .into_iter()
            .map(|(receipt_id, transaction_hash)| ReceiptTxHashMapRow {
                receipt_id,
                transaction_hash,
                block_height: facts.block_height,
            })
            .collect::<Vec<_>>();
        for row in &mapped_receipt_rows {
            receipt_tx_hashes
                .entry(row.receipt_id.clone())
                .or_insert_with(|| row.transaction_hash.clone());
        }
        metrics.record_phase_elapsed("db_lookup", db_lookup_start);

        {
            let mut state = self.state.lock().unwrap();
            state.remember_receipt_tx_hashes(
                facts.block_height,
                &mapped_receipt_rows,
                &mapped_receipt_rows,
            );
        }

        lookup_stats.db_hit_count = mapped_receipt_rows.len();
        lookup_stats.db_miss_count = unresolved_receipt_ids
            .len()
            .saturating_sub(lookup_stats.db_hit_count);
        metrics.record_count("receipt_lookup_candidate_count", lookup_stats.candidate_count);
        metrics.record_count(
            "receipt_lookup_local_resolved_count",
            lookup_stats.local_resolved_count,
        );
        metrics.record_count(
            "receipt_lookup_hot_cache_hit_count",
            lookup_stats.hot_cache_hit_count,
        );
        metrics.record_count(
            "receipt_lookup_db_receipt_count",
            unresolved_receipt_ids.len(),
        );
        metrics.record_count("receipt_lookup_db_query_count", lookup_stats.db_query_count);
        metrics.record_count("receipt_lookup_db_hit_count", lookup_stats.db_hit_count);
        metrics.record_count("receipt_lookup_db_miss_count", lookup_stats.db_miss_count);

        Ok(ResolvedReceiptGraph {
            receipt_tx_hashes,
            data_receipt_links,
            unresolved_receipt_ids,
            data_receipt_link_cache_size,
            metrics,
            lookup_stats,
        })
    }

    pub async fn resolve_from_canonical(
        &self,
        db: &impl ConnectionTrait,
        unresolved_receipt_ids: &[String],
        block_height: i64,
    ) -> Result<Vec<ReceiptTxHashMapRow>> {
        let resolved = receipt_tx_canonical_resolver::resolve_receipt_tx_hashes_from_canonical_graph(
            db,
            unresolved_receipt_ids,
        )
        .await?;

        Ok(resolved
            .into_iter()
            .map(|(receipt_id, transaction_hash)| ReceiptTxHashMapRow {
                receipt_id,
                transaction_hash,
                block_height,
            })
            .collect())
    }

    pub fn plan_upserts(
        &self,
        rows: &[ReceiptTxHashMapRow],
        block_height: i64,
        mode: ReceiptGraphUpsertMode,
    ) -> (Vec<ReceiptTxHashMapRow>, ReceiptGraphWriteStats) {
        let state = self.state.lock().unwrap();
        state.plan_receipt_tx_hash_map_writes(rows, block_height, mode)
    }

    pub fn remember_after_commit(
        &self,
        block_height: i64,
        new_data_receipt_links: HashMap<String, DataReceiptParentLink>,
        rows: &[ReceiptTxHashMapRow],
        persisted_rows: &[ReceiptTxHashMapRow],
    ) {
        let mut state = self.state.lock().unwrap();
        state.remember_data_receipt_links(block_height, new_data_receipt_links);
        state.remember_receipt_tx_hashes(block_height, rows, persisted_rows);
    }

    pub fn evict_receipt_tx_hot_cache(
        &self,
        block_height: i64,
        retention_blocks: i64,
        active_graph_tx_hashes: &HashSet<String>,
    ) {
        let mut state = self.state.lock().unwrap();
        state.evict_receipt_tx_hot_cache(block_height, retention_blocks, active_graph_tx_hashes);
    }

    pub fn hot_cache_size(&self) -> usize {
        let state = self.state.lock().unwrap();
        state.receipt_tx_hot_cache.len()
    }

    pub fn note_receipt_tx_hash_mapping(
        mappings: &mut HashMap<String, ReceiptTxHashMapRow>,
        receipt_id: &str,
        transaction_hash: &str,
        block_height: i64,
        module_name: &str,
    ) {
        mappings
            .entry(receipt_id.to_string())
            .and_modify(|row| {
                if row.transaction_hash != transaction_hash {
                    error!(
                        module = module_name,
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

    pub fn note_output_data_receivers(
        links: &mut HashMap<String, DataReceiptParentLink>,
        receipt: &ReceiptView,
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

    pub fn data_receipt_link<'a>(
        links: &'a HashMap<String, DataReceiptParentLink>,
        receipt: &ReceiptView,
    ) -> Option<&'a DataReceiptParentLink> {
        let ReceiptEnumView::Data { data_id, .. } = &receipt.receipt else {
            return None;
        };
        links.get(&data_id.to_string())
    }

    fn seed_receipt_tx_hashes_from_data_links(
        msg: &StreamerMessage,
        data_receipt_links: &HashMap<String, DataReceiptParentLink>,
        receipt_tx_hashes: &mut HashMap<String, String>,
    ) {
        for shard in &msg.shards {
            if let Some(chunk) = &shard.chunk {
                for receipt in chunk.receipts.iter().chain(chunk.local_receipts.iter()) {
                    if let Some(link) = Self::data_receipt_link(data_receipt_links, receipt) {
                        if let Some(transaction_hash) = link.transaction_hash.as_ref() {
                            receipt_tx_hashes
                                .entry(receipt.receipt_id.to_string())
                                .or_insert_with(|| transaction_hash.clone());
                        }
                    }
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                if let Some(link) = Self::data_receipt_link(data_receipt_links, receipt) {
                    if let Some(transaction_hash) = link.transaction_hash.as_ref() {
                        receipt_tx_hashes
                            .entry(receipt.receipt_id.to_string())
                            .or_insert_with(|| transaction_hash.clone());
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ReceiptGraphRuntimeState, ReceiptGraphUpsertMode, ReceiptTxHashMapRow,
        RECEIPT_TX_MAP_REFRESH_INTERVAL_BLOCKS,
    };
    use std::collections::HashMap;

    #[test]
    fn receipt_tx_hot_cache_resolves_misses_without_db() {
        let mut state = ReceiptGraphRuntimeState::default();
        state.remember_receipt_tx_hashes(
            100,
            &[ReceiptTxHashMapRow {
                receipt_id: "receipt-1".to_string(),
                transaction_hash: "tx-1".to_string(),
                block_height: 100,
            }],
            &[ReceiptTxHashMapRow {
                receipt_id: "receipt-1".to_string(),
                transaction_hash: "tx-1".to_string(),
                block_height: 100,
            }],
        );

        let mut receipt_tx_hashes = HashMap::new();
        let stats = state.resolve_receipt_tx_hashes_from_hot_cache(
            101,
            &["receipt-1".to_string(), "receipt-2".to_string()],
            &mut receipt_tx_hashes,
        );

        assert_eq!(
            receipt_tx_hashes.get("receipt-1"),
            Some(&"tx-1".to_string())
        );
        assert_eq!(stats.candidate_count, 2);
        assert_eq!(stats.hot_cache_hit_count, 1);
        assert_eq!(stats.local_resolved_count, 0);
    }

    #[test]
    fn receipt_tx_map_write_plan_skips_recent_known_rows() {
        let mut state = ReceiptGraphRuntimeState::default();
        state.remember_receipt_tx_hashes(
            100,
            &[ReceiptTxHashMapRow {
                receipt_id: "receipt-1".to_string(),
                transaction_hash: "tx-1".to_string(),
                block_height: 100,
            }],
            &[ReceiptTxHashMapRow {
                receipt_id: "receipt-1".to_string(),
                transaction_hash: "tx-1".to_string(),
                block_height: 100,
            }],
        );

        let (planned, stats) = state.plan_receipt_tx_hash_map_writes(
            &[ReceiptTxHashMapRow {
                receipt_id: "receipt-1".to_string(),
                transaction_hash: "tx-1".to_string(),
                block_height: 101,
            }],
            101,
            ReceiptGraphUpsertMode::Venear,
        );

        assert!(planned.is_empty());
        assert_eq!(stats.new_count, 0);
        assert_eq!(stats.refresh_count, 0);
        assert_eq!(stats.skipped_count, 1);
    }

    #[test]
    fn receipt_tx_map_write_plan_refreshes_stale_rows() {
        let mut state = ReceiptGraphRuntimeState::default();
        state.remember_receipt_tx_hashes(
            100,
            &[ReceiptTxHashMapRow {
                receipt_id: "receipt-1".to_string(),
                transaction_hash: "tx-1".to_string(),
                block_height: 100,
            }],
            &[ReceiptTxHashMapRow {
                receipt_id: "receipt-1".to_string(),
                transaction_hash: "tx-1".to_string(),
                block_height: 100,
            }],
        );

        let (planned, stats) = state.plan_receipt_tx_hash_map_writes(
            &[ReceiptTxHashMapRow {
                receipt_id: "receipt-1".to_string(),
                transaction_hash: "tx-1".to_string(),
                block_height: 100 + RECEIPT_TX_MAP_REFRESH_INTERVAL_BLOCKS,
            }],
            100 + RECEIPT_TX_MAP_REFRESH_INTERVAL_BLOCKS,
            ReceiptGraphUpsertMode::Venear,
        );

        assert_eq!(planned.len(), 1);
        assert_eq!(stats.new_count, 0);
        assert_eq!(stats.refresh_count, 1);
        assert_eq!(stats.skipped_count, 0);
    }
}
