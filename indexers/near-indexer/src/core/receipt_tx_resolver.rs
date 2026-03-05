use near_lake_framework::near_indexer_primitives::IndexerShard;
use std::collections::hash_map::Entry;
use std::collections::HashMap;

/// Builds a best-effort `receipt_id -> transaction_hash` map from a block payload.
///
/// FastNEAR payloads often omit `tx_with_outcome.outcome.receipt` but still include
/// root receipt ids in `tx_with_outcome.outcome.execution_outcome.outcome.receipt_ids`.
/// This resolver seeds from both locations, then propagates hashes through the
/// parent->child receipt graph from `receipt_execution_outcomes`.
pub fn build_receipt_tx_hashes(shards: &[IndexerShard]) -> HashMap<String, String> {
    let mut receipt_tx_hashes: HashMap<String, String> = HashMap::new();

    for shard in shards {
        if let Some(chunk) = &shard.chunk {
            for tx_with_outcome in &chunk.transactions {
                let tx_hash = tx_with_outcome.transaction.hash.to_string();

                if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                    receipt_tx_hashes
                        .entry(receipt.receipt_id.to_string())
                        .or_insert_with(|| tx_hash.clone());
                }

                for child_receipt_id in &tx_with_outcome
                    .outcome
                    .execution_outcome
                    .outcome
                    .receipt_ids
                {
                    receipt_tx_hashes
                        .entry(child_receipt_id.to_string())
                        .or_insert_with(|| tx_hash.clone());
                }
            }
        }
    }

    let mut changed = true;
    while changed {
        changed = false;

        for shard in shards {
            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let parent_receipt_id = outcome_with_receipt.receipt.receipt_id.to_string();
                let Some(parent_tx_hash) = receipt_tx_hashes.get(&parent_receipt_id).cloned()
                else {
                    continue;
                };

                for child_receipt_id in &outcome_with_receipt.execution_outcome.outcome.receipt_ids
                {
                    let child_receipt_id = child_receipt_id.to_string();
                    if let Entry::Vacant(entry) = receipt_tx_hashes.entry(child_receipt_id) {
                        entry.insert(parent_tx_hash.clone());
                        changed = true;
                    }
                }
            }
        }
    }

    receipt_tx_hashes
}
