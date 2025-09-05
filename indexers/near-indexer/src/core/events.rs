use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Try to parse an `EVENT_JSON:` log line into a near-lake-primitives RawEvent.
/// Returns `None` if the log is not an event or parsing fails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawEvent {
    pub standard: String,
    pub version: String,
    pub event: String,
    pub data: serde_json::Value,
}

pub fn parse_raw_event_from_log(log: &str) -> Option<RawEvent> {
    if !log.starts_with("EVENT_JSON:") {
        return None;
    }
    let json_str = &log["EVENT_JSON:".len()..];
    serde_json::from_str::<RawEvent>(json_str).ok()
}

/// Collect RawEvents from a given ExecutionOutcome logs list.
pub fn parse_raw_events_from_logs<'a>(logs: impl IntoIterator<Item = &'a String>) -> Vec<RawEvent> {
    logs.into_iter()
        .filter_map(|l| parse_raw_event_from_log(l))
        .collect()
}

/// Convenience: parse events from a tx's execution outcome (common path) and
/// from a receipt execution outcome. Callers can then map to domain models.
pub fn outcome_events(
    outcome: &near_lake_framework::near_indexer_primitives::views::ExecutionOutcomeWithIdView,
) -> Vec<RawEvent> {
    parse_raw_events_from_logs(&outcome.outcome.logs)
}

/// Enriched event carrying useful linkage for downstream processing.
#[derive(Debug, Clone)]
pub struct BlockEvent {
    pub raw: RawEvent,
    pub tx_hash: String,
    pub receipt_id: String,
    pub executor_id: String,
}

/// Collect all events within a full StreamerMessage (transactions + receipt outcomes),
/// attaching the originating `tx_hash`, `receipt_id`, and `executor_id`.
pub fn collect_block_events(
    msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
) -> Vec<BlockEvent> {
    // no-op: keep API similar; nothing to import here
    

    // Map receipt_id -> tx_hash from chunk transactions
    let mut receipt_to_tx: HashMap<String, String> = HashMap::new();
    let mut results: Vec<BlockEvent> = Vec::new();

    for shard in &msg.shards {
        if let Some(chunk) = &shard.chunk {
            for txo in &chunk.transactions {
                let tx_hash = txo.transaction.hash.to_string();
                let outcome = &txo.outcome.execution_outcome;

                // Map receipt ids to tx
                for rid in &outcome.outcome.receipt_ids {
                    receipt_to_tx.insert(rid.to_string(), tx_hash.clone());
                }

                // Events directly on transaction outcome
                for raw in parse_raw_events_from_logs(&outcome.outcome.logs) {
                    results.push(BlockEvent {
                        tx_hash: tx_hash.clone(),
                        receipt_id: tx_hash.clone(), // tx outcome: use tx hash as receipt surrogate
                        executor_id: outcome.outcome.executor_id.to_string(),
                        raw,
                    });
                }
            }
        }
    }

    // Receipt execution outcomes
    for shard in &msg.shards {
        for reo in &shard.receipt_execution_outcomes {
            let receipt_id = reo.receipt.receipt_id.to_string();
            let tx_hash = receipt_to_tx
                .get(&receipt_id)
                .cloned()
                .unwrap_or_else(|| receipt_id.clone());

            for raw in parse_raw_events_from_logs(&reo.execution_outcome.outcome.logs) {
                results.push(BlockEvent {
                    tx_hash: tx_hash.clone(),
                    receipt_id: receipt_id.clone(),
                    executor_id: reo.execution_outcome.outcome.executor_id.to_string(),
                    raw,
                });
            }
        }
    }

    results
}
