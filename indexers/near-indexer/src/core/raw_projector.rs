use anyhow::Result;
use near_lake_framework::near_indexer_primitives::IndexerShard;
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::ConnectionTrait;
use std::collections::HashMap;

use crate::core::block_facts::BlockFacts;
use crate::core::canonical_rows::{
    self, ExecutionOutcomeRowMeta, NearBlockRow, NearChunkRow, NearExecutionOutcomeRow,
    NearReceiptActionRow, NearReceiptEdgeRow, NearReceiptRow, NearStateChangesRow,
    NearTransactionRow, ReceiptActionRowMeta, ReceiptTxHashMapRow,
};
use crate::core::receipt_graph_resolver::{
    DataReceiptParentLink, ReceiptGraphResolver, ResolvedReceiptGraph,
};

const MODULE_NAME: &str = "raw";

#[derive(Clone)]
pub struct RawBlockProjection {
    pub block_row: NearBlockRow,
    pub chunk_rows: Vec<NearChunkRow>,
    pub transaction_rows: Vec<NearTransactionRow>,
    pub receipt_rows: Vec<NearReceiptRow>,
    pub state_change_rows: Vec<NearStateChangesRow>,
    pub execution_outcome_rows: Vec<NearExecutionOutcomeRow>,
    pub receipt_action_rows: Vec<NearReceiptActionRow>,
    pub receipt_edge_rows: Vec<NearReceiptEdgeRow>,
    pub receipt_tx_rows: Vec<ReceiptTxHashMapRow>,
    pub new_data_receipt_links: HashMap<String, DataReceiptParentLink>,
}

pub struct RawProjector;

impl RawProjector {
    pub async fn project(
        msg: &StreamerMessage,
        facts: &BlockFacts,
        resolved: &ResolvedReceiptGraph,
        db: &impl ConnectionTrait,
    ) -> Result<RawBlockProjection> {
        let mut chunk_rows = Vec::new();
        let mut transaction_rows = Vec::new();
        let mut receipt_rows = Vec::new();
        let mut state_change_rows = Vec::new();
        let mut seen_receipt_rows = std::collections::HashSet::new();

        for shard in &msg.shards {
            let shard_id = canonical_rows::shard_to_i64(shard.shard_id);

            if let Some(chunk) = &shard.chunk {
                chunk_rows.push(canonical_rows::chunk_to_row(
                    chunk,
                    facts.block_height,
                    shard_id,
                )?);

                for tx_with_outcome in &chunk.transactions {
                    transaction_rows.push(canonical_rows::transaction_to_row(
                        &tx_with_outcome.transaction,
                        facts.block_height,
                        shard_id,
                    )?);

                    if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                        Self::push_receipt_row(
                            &mut receipt_rows,
                            &mut seen_receipt_rows,
                            receipt,
                            facts.block_height,
                            shard_id,
                        )?;
                    }
                }

                for receipt in chunk.receipts.iter().chain(chunk.local_receipts.iter()) {
                    Self::push_receipt_row(
                        &mut receipt_rows,
                        &mut seen_receipt_rows,
                        receipt,
                        facts.block_height,
                        shard_id,
                    )?;
                }
            }

            if !shard.state_changes.is_empty() {
                state_change_rows.push(canonical_rows::state_changes_to_row(
                    &shard.state_changes,
                    facts.block_height,
                    shard_id,
                )?);
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                Self::push_receipt_row(
                    &mut receipt_rows,
                    &mut seen_receipt_rows,
                    &outcome_with_receipt.receipt,
                    facts.block_height,
                    shard_id,
                )?;
            }
        }

        let mut resolved_graph = resolved.clone();
        let canonical_rows = ReceiptGraphResolver::resolve_from_canonical(
            db,
            &resolved_graph.unresolved_receipt_ids,
            facts.block_height,
        )
        .await?;
        let mut receipt_tx_rows = HashMap::new();
        for row in canonical_rows {
            ReceiptGraphResolver::note_receipt_tx_hash_mapping(
                &mut receipt_tx_rows,
                &row.receipt_id,
                &row.transaction_hash,
                facts.block_height,
                MODULE_NAME,
            );
            resolved_graph
                .receipt_tx_hashes
                .entry(row.receipt_id)
                .or_insert(row.transaction_hash);
        }

        let execution = Self::project_execution_outcomes(
            &msg.shards,
            facts,
            &mut resolved_graph.receipt_tx_hashes,
            &mut resolved_graph.data_receipt_links,
            &mut receipt_tx_rows,
        )?;

        Ok(RawBlockProjection {
            block_row: canonical_rows::block_to_row(&msg.block, &facts.block_author)?,
            chunk_rows,
            transaction_rows,
            receipt_rows,
            state_change_rows,
            execution_outcome_rows: execution.execution_outcome_rows,
            receipt_action_rows: execution.receipt_action_rows,
            receipt_edge_rows: execution.receipt_edge_rows,
            receipt_tx_rows: receipt_tx_rows.into_values().collect(),
            new_data_receipt_links: execution.new_data_receipt_links,
        })
    }

    fn project_execution_outcomes(
        shards: &[IndexerShard],
        facts: &BlockFacts,
        receipt_tx_hashes: &mut HashMap<String, String>,
        data_receipt_links: &mut HashMap<String, DataReceiptParentLink>,
        receipt_tx_rows: &mut HashMap<String, ReceiptTxHashMapRow>,
    ) -> Result<ExecutionProjection> {
        let mut new_data_receipt_links = HashMap::new();
        let mut outcome_rows = HashMap::new();
        let mut edge_rows = HashMap::new();
        let mut action_rows = HashMap::new();

        for shard in shards {
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    let Some(receipt) = &tx_with_outcome.outcome.receipt else {
                        continue;
                    };
                    let transaction_hash = tx_with_outcome.transaction.hash.to_string();
                    let execution_outcome_id =
                        tx_with_outcome.outcome.execution_outcome.id.to_string();
                    ReceiptGraphResolver::note_output_data_receivers(
                        data_receipt_links,
                        receipt,
                        Some(&transaction_hash),
                        &execution_outcome_id,
                        facts.block_height,
                        &facts.block_hash,
                        facts.block_timestamp,
                    );
                    ReceiptGraphResolver::note_output_data_receivers(
                        &mut new_data_receipt_links,
                        receipt,
                        Some(&transaction_hash),
                        &execution_outcome_id,
                        facts.block_height,
                        &facts.block_hash,
                        facts.block_timestamp,
                    );
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                let receipt = &outcome_with_receipt.receipt;
                let receipt_id = receipt.receipt_id.to_string();
                let data_receipt_link =
                    ReceiptGraphResolver::data_receipt_link(data_receipt_links, receipt).cloned();
                let mut transaction_hash = receipt_tx_hashes.get(&receipt_id).cloned();
                if transaction_hash.is_none() {
                    transaction_hash = data_receipt_link
                        .as_ref()
                        .and_then(|link| link.transaction_hash.clone());
                }
                let execution_outcome_id = outcome_with_receipt.execution_outcome.id.to_string();

                ReceiptGraphResolver::note_output_data_receivers(
                    data_receipt_links,
                    receipt,
                    transaction_hash.as_deref(),
                    &execution_outcome_id,
                    facts.block_height,
                    &facts.block_hash,
                    facts.block_timestamp,
                );
                ReceiptGraphResolver::note_output_data_receivers(
                    &mut new_data_receipt_links,
                    receipt,
                    transaction_hash.as_deref(),
                    &execution_outcome_id,
                    facts.block_height,
                    &facts.block_hash,
                    facts.block_timestamp,
                );
            }
        }

        for (shard, shard_fact) in shards.iter().zip(facts.shards.iter()) {
            let shard_id = shard_fact.shard_id;
            let chunk_hash = shard_fact.chunk_hash.clone();
            let chunk_author = shard_fact.chunk_author.clone();

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
                                block_height: facts.block_height,
                                shard_id,
                                receipt_id: receipt_id.as_deref(),
                                block_hash: &facts.block_hash,
                                chunk_hash: &chunk_hash,
                                block_timestamp: facts.block_timestamp,
                            },
                        )?,
                    );

                    if let Some(parent_receipt_id) = receipt_id.as_deref() {
                        receipt_tx_hashes
                            .entry(parent_receipt_id.to_string())
                            .or_insert_with(|| transaction_hash.clone());
                        ReceiptGraphResolver::note_receipt_tx_hash_mapping(
                            receipt_tx_rows,
                            parent_receipt_id,
                            &transaction_hash,
                            facts.block_height,
                            MODULE_NAME,
                        );

                        for child_receipt_id in &outcome.receipt_ids {
                            let child_receipt_id_str = child_receipt_id.to_string();
                            edge_rows.insert(
                                (parent_receipt_id.to_string(), child_receipt_id_str.clone()),
                                canonical_rows::NearReceiptEdgeRow {
                                    parent_receipt_id: parent_receipt_id.to_string(),
                                    child_receipt_id: child_receipt_id_str.clone(),
                                    parent_execution_outcome_id: execution_outcome_id.clone(),
                                    block_height: facts.block_height,
                                    block_hash: facts.block_hash.clone(),
                                    block_timestamp: facts.block_timestamp,
                                    transaction_hash: Some(transaction_hash.clone()),
                                },
                            );

                            receipt_tx_hashes
                                .entry(child_receipt_id_str.clone())
                                .or_insert_with(|| transaction_hash.clone());
                            ReceiptGraphResolver::note_receipt_tx_hash_mapping(
                                receipt_tx_rows,
                                &child_receipt_id_str,
                                &transaction_hash,
                                facts.block_height,
                                MODULE_NAME,
                            );
                        }
                    } else {
                        for child_receipt_id in &outcome.receipt_ids {
                            let child_receipt_id_str = child_receipt_id.to_string();
                            receipt_tx_hashes
                                .entry(child_receipt_id_str.clone())
                                .or_insert_with(|| transaction_hash.clone());
                            ReceiptGraphResolver::note_receipt_tx_hash_mapping(
                                receipt_tx_rows,
                                &child_receipt_id_str,
                                &transaction_hash,
                                facts.block_height,
                                MODULE_NAME,
                            );
                        }
                    }
                }

                for receipt in chunk.receipts.iter().chain(chunk.local_receipts.iter()) {
                    let receipt_id = receipt.receipt_id.to_string();
                    let data_receipt_link =
                        ReceiptGraphResolver::data_receipt_link(data_receipt_links, receipt)
                            .cloned();
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
                        ReceiptGraphResolver::note_receipt_tx_hash_mapping(
                            receipt_tx_rows,
                            &receipt_id,
                            transaction_hash,
                            facts.block_height,
                            MODULE_NAME,
                        );
                    }

                    if let Some(link) = data_receipt_link.as_ref() {
                        edge_rows.insert(
                            (link.parent_receipt_id.clone(), receipt_id.clone()),
                            canonical_rows::NearReceiptEdgeRow {
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
                    ReceiptGraphResolver::data_receipt_link(data_receipt_links, receipt).cloned();
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
                            block_height: facts.block_height,
                            shard_id,
                            receipt_id: Some(&receipt_id),
                            block_hash: &facts.block_hash,
                            chunk_hash: &chunk_hash,
                            block_timestamp: facts.block_timestamp,
                        },
                    )?,
                );

                if let Some(transaction_hash) = &transaction_hash {
                    receipt_tx_hashes
                        .entry(receipt_id.clone())
                        .or_insert_with(|| transaction_hash.clone());
                    ReceiptGraphResolver::note_receipt_tx_hash_mapping(
                        receipt_tx_rows,
                        &receipt_id,
                        transaction_hash,
                        facts.block_height,
                        MODULE_NAME,
                    );
                }

                if let Some(link) = data_receipt_link.as_ref() {
                    edge_rows.insert(
                        (link.parent_receipt_id.clone(), receipt_id.clone()),
                        canonical_rows::NearReceiptEdgeRow {
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
                        canonical_rows::NearReceiptEdgeRow {
                            parent_receipt_id: receipt_id.clone(),
                            child_receipt_id: child_receipt_id_str.clone(),
                            parent_execution_outcome_id: execution_outcome_id.clone(),
                            block_height: facts.block_height,
                            block_hash: facts.block_hash.clone(),
                            block_timestamp: facts.block_timestamp,
                            transaction_hash: transaction_hash.clone(),
                        },
                    );

                    if let Some(transaction_hash) = &transaction_hash {
                        receipt_tx_hashes
                            .entry(child_receipt_id_str.clone())
                            .or_insert_with(|| transaction_hash.clone());
                        ReceiptGraphResolver::note_receipt_tx_hash_mapping(
                            receipt_tx_rows,
                            &child_receipt_id_str,
                            transaction_hash,
                            facts.block_height,
                            MODULE_NAME,
                        );
                    }
                }

                for row in canonical_rows::receipt_action_rows(
                    receipt,
                    ReceiptActionRowMeta {
                        transaction_hash: transaction_hash.as_deref(),
                        block_height: facts.block_height,
                        execution_outcome_id: &execution_outcome_id,
                        shard_id,
                        block_hash: &facts.block_hash,
                        chunk_hash: &chunk_hash,
                        author: &chunk_author,
                        block_timestamp: facts.block_timestamp,
                    },
                )? {
                    action_rows.insert(row.id.clone(), row);
                }
            }
        }

        Ok(ExecutionProjection {
            execution_outcome_rows: outcome_rows.into_values().collect(),
            receipt_edge_rows: edge_rows.into_values().collect(),
            receipt_action_rows: action_rows.into_values().collect(),
            new_data_receipt_links,
        })
    }

    fn push_receipt_row(
        rows: &mut Vec<NearReceiptRow>,
        seen_receipt_rows: &mut std::collections::HashSet<String>,
        receipt: &near_lake_framework::near_indexer_primitives::views::ReceiptView,
        block_height: i64,
        shard_id: i64,
    ) -> Result<()> {
        let receipt_id = receipt.receipt_id.to_string();
        if seen_receipt_rows.insert(receipt_id) {
            rows.push(canonical_rows::receipt_to_row(
                receipt,
                block_height,
                shard_id,
            )?);
        }
        Ok(())
    }
}

struct ExecutionProjection {
    execution_outcome_rows: Vec<NearExecutionOutcomeRow>,
    receipt_action_rows: Vec<NearReceiptActionRow>,
    receipt_edge_rows: Vec<NearReceiptEdgeRow>,
    new_data_receipt_links: HashMap<String, DataReceiptParentLink>,
}
