use anyhow::Result;
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::core::canonical_rows;
use crate::core::receipt_tx_resolver::build_receipt_tx_hashes;
use crate::core::stage_timing::StageMetrics;

#[derive(Clone)]
pub struct BlockShardFact {
    pub shard_id: i64,
    pub chunk_hash: String,
    pub chunk_author: String,
}

#[derive(Clone)]
pub struct BlockFacts {
    pub block_height: i64,
    pub prev_height: Option<i64>,
    pub block_hash: String,
    pub block_timestamp: i64,
    pub block_author: String,
    pub shard_count: usize,
    pub chunk_count: usize,
    pub transaction_count: usize,
    pub receipt_count: usize,
    pub receipt_execution_outcome_count: usize,
    pub state_change_count: usize,
    pub candidate_receipt_ids: Vec<String>,
    pub candidate_receipt_id_count_before_dedup: usize,
    pub data_receipt_ids: HashSet<String>,
    pub receipt_tx_hashes: HashMap<String, String>,
    pub shards: Vec<BlockShardFact>,
}

pub struct BlockFactsBuild {
    pub facts: BlockFacts,
    pub metrics: StageMetrics,
}

impl BlockFacts {
    pub fn from_message(msg: &StreamerMessage) -> Result<Self> {
        Ok(Self::from_message_with_metrics(msg)?.facts)
    }

    pub fn from_message_with_metrics(msg: &StreamerMessage) -> Result<BlockFactsBuild> {
        let mut metrics = StageMetrics::new();
        let block_height = msg.block.header.height as i64;
        let prev_height = msg.block.header.prev_height.map(|value| value as i64);
        let block_hash = msg.block.header.hash.to_string();
        let block_timestamp = msg.block.header.timestamp as i64;
        let block_author = msg.block.author.to_string();

        let mut shards = Vec::with_capacity(msg.shards.len());
        let mut candidate_receipt_ids = Vec::new();
        let mut seen_candidate_receipt_ids = HashSet::new();
        let mut candidate_receipt_id_count_before_dedup = 0usize;
        let mut data_receipt_ids = HashSet::new();

        let collect_facts_start = Instant::now();
        for shard in &msg.shards {
            let shard_id = canonical_rows::shard_to_i64(shard.shard_id);
            let chunk_hash = shard
                .chunk
                .as_ref()
                .map(|chunk| chunk.header.chunk_hash.to_string())
                .unwrap_or_default();
            let chunk_author = shard
                .chunk
                .as_ref()
                .map(|chunk| chunk.author.to_string())
                .unwrap_or_else(|| block_author.clone());
            shards.push(BlockShardFact {
                shard_id,
                chunk_hash,
                chunk_author,
            });

            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        Self::push_candidate_receipt_id(
                            &mut candidate_receipt_ids,
                            &mut seen_candidate_receipt_ids,
                            &mut candidate_receipt_id_count_before_dedup,
                            &receipt.receipt_id.to_string(),
                        );
                    }

                    for child_receipt_id in &tx_with_outcome
                        .outcome
                        .execution_outcome
                        .outcome
                        .receipt_ids
                    {
                        Self::push_candidate_receipt_id(
                            &mut candidate_receipt_ids,
                            &mut seen_candidate_receipt_ids,
                            &mut candidate_receipt_id_count_before_dedup,
                            &child_receipt_id.to_string(),
                        );
                    }
                }

                for receipt in chunk.receipts.iter().chain(chunk.local_receipts.iter()) {
                    Self::push_candidate_receipt_id(
                        &mut candidate_receipt_ids,
                        &mut seen_candidate_receipt_ids,
                        &mut candidate_receipt_id_count_before_dedup,
                        &receipt.receipt_id.to_string(),
                    );
                    Self::push_data_receipt_id(&mut data_receipt_ids, receipt);
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                Self::push_candidate_receipt_id(
                    &mut candidate_receipt_ids,
                    &mut seen_candidate_receipt_ids,
                    &mut candidate_receipt_id_count_before_dedup,
                    &receipt.receipt_id.to_string(),
                );
                Self::push_data_receipt_id(&mut data_receipt_ids, receipt);

                for child_receipt_id in &outcome_with_receipt.execution_outcome.outcome.receipt_ids
                {
                    Self::push_candidate_receipt_id(
                        &mut candidate_receipt_ids,
                        &mut seen_candidate_receipt_ids,
                        &mut candidate_receipt_id_count_before_dedup,
                        &child_receipt_id.to_string(),
                    );
                }
            }
        }
        metrics.record_phase_elapsed("collect_facts", collect_facts_start);

        let build_receipt_tx_hashes_start = Instant::now();
        let receipt_tx_hashes = build_receipt_tx_hashes(&msg.shards);
        metrics.record_phase_elapsed("build_receipt_tx_hashes", build_receipt_tx_hashes_start);

        let facts = Self {
            block_height,
            prev_height,
            block_hash,
            block_timestamp,
            block_author,
            shard_count: msg.shards.len(),
            chunk_count: msg
                .shards
                .iter()
                .filter(|shard| shard.chunk.is_some())
                .count(),
            transaction_count: msg
                .shards
                .iter()
                .filter_map(|shard| shard.chunk.as_ref())
                .map(|chunk| chunk.transactions.len())
                .sum(),
            receipt_count: msg
                .shards
                .iter()
                .filter_map(|shard| shard.chunk.as_ref())
                .map(|chunk| chunk.receipts.len() + chunk.local_receipts.len())
                .sum(),
            receipt_execution_outcome_count: msg
                .shards
                .iter()
                .map(|shard| shard.receipt_execution_outcomes.len())
                .sum(),
            state_change_count: msg
                .shards
                .iter()
                .map(|shard| shard.state_changes.len())
                .sum(),
            candidate_receipt_ids,
            candidate_receipt_id_count_before_dedup,
            data_receipt_ids,
            receipt_tx_hashes,
            shards,
        };

        metrics.record_count(
            "candidate_receipt_ids_before_dedup",
            facts.candidate_receipt_id_count_before_dedup,
        );
        metrics.record_count(
            "candidate_receipt_ids_after_dedup",
            facts.candidate_receipt_ids.len(),
        );
        metrics.record_count("data_receipt_id_count", facts.data_receipt_ids.len());
        metrics.record_count("receipt_tx_seed_count", facts.receipt_tx_hashes.len());
        metrics.record_count("shard_count", facts.shard_count);
        metrics.record_count("chunk_count", facts.chunk_count);
        metrics.record_count("transaction_count", facts.transaction_count);
        metrics.record_count("receipt_count", facts.receipt_count);
        metrics.record_count(
            "receipt_execution_outcome_count",
            facts.receipt_execution_outcome_count,
        );
        metrics.record_count("state_change_count", facts.state_change_count);

        Ok(BlockFactsBuild { facts, metrics })
    }

    fn push_candidate_receipt_id(
        candidate_receipt_ids: &mut Vec<String>,
        seen_candidate_receipt_ids: &mut HashSet<String>,
        candidate_receipt_id_count_before_dedup: &mut usize,
        receipt_id: &str,
    ) {
        *candidate_receipt_id_count_before_dedup += 1;
        if seen_candidate_receipt_ids.insert(receipt_id.to_string()) {
            candidate_receipt_ids.push(receipt_id.to_string());
        }
    }

    fn push_data_receipt_id(
        data_receipt_ids: &mut HashSet<String>,
        receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
    ) {
        if let near_lake_framework::near_indexer_primitives::views::ReceiptEnumView::Data {
            data_id,
            ..
        } = &receipt.receipt
        {
            data_receipt_ids.insert(data_id.to_string());
        }
    }
}
