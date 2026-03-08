use anyhow::Result;
use near_lake_framework::near_indexer_primitives::views::{ReceiptEnumView, ReceiptView};
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::core::canonical_rows::{
    self, NearBlockRow, NearChunkRow, NearReceiptRow, NearStateChangesRow, NearTransactionRow,
};
use crate::core::receipt_tx_resolver::build_receipt_tx_hashes;
use crate::core::stage_timing::StageMetrics;

#[derive(Clone)]
pub struct PreparedShardContext {
    pub shard_id: i64,
    pub chunk_hash: String,
    pub chunk_author: String,
}

pub struct PreparedBlockContext {
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
    pub block_row: NearBlockRow,
    pub chunk_rows: Vec<NearChunkRow>,
    pub transaction_rows: Vec<NearTransactionRow>,
    pub receipt_rows: Vec<NearReceiptRow>,
    pub state_change_rows: Vec<NearStateChangesRow>,
    pub shards: Vec<PreparedShardContext>,
}

pub struct PreparedBlockBuild {
    pub context: PreparedBlockContext,
    pub metrics: StageMetrics,
}

impl PreparedBlockContext {
    pub fn from_message(msg: &StreamerMessage) -> Result<Self> {
        Ok(Self::from_message_with_metrics(msg)?.context)
    }

    pub fn from_message_with_metrics(msg: &StreamerMessage) -> Result<PreparedBlockBuild> {
        let mut metrics = StageMetrics::new();
        let block_height = msg.block.header.height as i64;
        let prev_height = msg.block.header.prev_height.map(|value| value as i64);
        let block_hash = msg.block.header.hash.to_string();
        let block_timestamp = msg.block.header.timestamp as i64;
        let block_author = msg.block.author.to_string();

        let mut chunk_rows = Vec::new();
        let mut transaction_rows = Vec::new();
        let mut receipt_rows = Vec::new();
        let mut state_change_rows = Vec::new();
        let mut shards = Vec::with_capacity(msg.shards.len());

        let mut candidate_receipt_ids = Vec::new();
        let mut seen_candidate_receipt_ids = HashSet::new();
        let mut candidate_receipt_id_count_before_dedup = 0usize;
        let mut data_receipt_ids = HashSet::new();
        let mut seen_receipt_rows = HashSet::new();

        let collect_rows_start = Instant::now();
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
            shards.push(PreparedShardContext {
                shard_id,
                chunk_hash,
                chunk_author,
            });

            if let Some(chunk) = &shard.chunk {
                chunk_rows.push(canonical_rows::chunk_to_row(chunk, block_height, shard_id)?);

                for tx_with_outcome in &chunk.transactions {
                    transaction_rows.push(canonical_rows::transaction_to_row(
                        &tx_with_outcome.transaction,
                        block_height,
                        shard_id,
                    )?);

                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        Self::push_candidate_receipt_id(
                            &mut candidate_receipt_ids,
                            &mut seen_candidate_receipt_ids,
                            &mut candidate_receipt_id_count_before_dedup,
                            &receipt.receipt_id.to_string(),
                        );
                        Self::push_receipt_row(
                            &mut receipt_rows,
                            &mut seen_receipt_rows,
                            receipt,
                            block_height,
                            shard_id,
                        )?;
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
                    Self::push_receipt_row(
                        &mut receipt_rows,
                        &mut seen_receipt_rows,
                        receipt,
                        block_height,
                        shard_id,
                    )?;
                }
            }

            if !shard.state_changes.is_empty() {
                state_change_rows.push(canonical_rows::state_changes_to_row(
                    &shard.state_changes,
                    block_height,
                    shard_id,
                )?);
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
                Self::push_receipt_row(
                    &mut receipt_rows,
                    &mut seen_receipt_rows,
                    receipt,
                    block_height,
                    shard_id,
                )?;

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

        metrics.record_phase_elapsed("collect_rows", collect_rows_start);

        let build_receipt_tx_hashes_start = Instant::now();
        let receipt_tx_hashes = build_receipt_tx_hashes(&msg.shards);
        metrics.record_phase_elapsed("build_receipt_tx_hashes", build_receipt_tx_hashes_start);

        let context = Self {
            block_height,
            prev_height,
            block_hash,
            block_timestamp,
            block_author: block_author.clone(),
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
            block_row: canonical_rows::block_to_row(&msg.block, &block_author)?,
            chunk_rows,
            transaction_rows,
            receipt_rows,
            state_change_rows,
            shards,
        };

        metrics.record_count(
            "candidate_receipt_ids_before_dedup",
            context.candidate_receipt_id_count_before_dedup,
        );
        metrics.record_count(
            "candidate_receipt_ids_after_dedup",
            context.candidate_receipt_ids.len(),
        );
        metrics.record_count("data_receipt_id_count", context.data_receipt_ids.len());
        metrics.record_count("receipt_tx_seed_count", context.receipt_tx_hashes.len());
        metrics.record_count("chunk_row_count", context.chunk_rows.len());
        metrics.record_count("transaction_row_count", context.transaction_rows.len());
        metrics.record_count("receipt_row_count", context.receipt_rows.len());
        metrics.record_count("state_change_row_count", context.state_change_rows.len());
        metrics.record_count("prepared_row_count", context.row_count());

        Ok(PreparedBlockBuild { context, metrics })
    }

    pub fn row_count(&self) -> usize {
        1 + self.chunk_rows.len()
            + self.transaction_rows.len()
            + self.receipt_rows.len()
            + self.state_change_rows.len()
    }

    fn push_candidate_receipt_id(
        ids: &mut Vec<String>,
        seen_ids: &mut HashSet<String>,
        original_count: &mut usize,
        receipt_id: &str,
    ) {
        *original_count += 1;
        if seen_ids.insert(receipt_id.to_string()) {
            ids.push(receipt_id.to_string());
        }
    }

    fn push_receipt_row(
        rows: &mut Vec<NearReceiptRow>,
        seen_receipt_ids: &mut HashSet<String>,
        receipt: &ReceiptView,
        block_height: i64,
        shard_id: i64,
    ) -> Result<()> {
        let receipt_id = receipt.receipt_id.to_string();
        if seen_receipt_ids.insert(receipt_id) {
            rows.push(canonical_rows::receipt_to_row(
                receipt,
                block_height,
                shard_id,
            )?);
        }
        Ok(())
    }

    fn push_data_receipt_id(data_receipt_ids: &mut HashSet<String>, receipt: &ReceiptView) {
        let ReceiptEnumView::Data { data_id, .. } = &receipt.receipt else {
            return;
        };
        data_receipt_ids.insert(data_id.to_string());
    }
}
