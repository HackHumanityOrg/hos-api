use anyhow::{Context, Result};
use async_trait::async_trait;
use common::config::NearModuleStreamMode;
use sea_orm::{DatabaseConnection, TransactionTrait};
use std::sync::Arc;
use tracing::{info, instrument};

use crate::core::block_artifacts::BlockArtifacts;
use crate::core::canonical_persistence::{self, NearReceiptWriteMode, NearTransactionWriteMode};
use crate::core::cursor;
use crate::core::module_demand::ModuleDemand;
use crate::core::raw_projector::RawBlockProjection;
use crate::core::receipt_graph_resolver::ReceiptGraphResolver;
use crate::core::stage_timing::BlockProcessingLog;
use crate::IndexerModule;

const MODULE_NAME: &str = "raw";

pub struct RawModule {
    enabled: bool,
    stream_mode: NearModuleStreamMode,
    receipt_graph: Arc<ReceiptGraphResolver>,
}

impl RawModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        Self::with_receipt_graph(config, Arc::new(ReceiptGraphResolver::new()))
    }

    pub fn with_receipt_graph(
        config: &common::config::AppConfig,
        receipt_graph: Arc<ReceiptGraphResolver>,
    ) -> Self {
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
            receipt_graph,
        }
    }

    async fn commit_projection(
        &self,
        projection: &RawBlockProjection,
        db: &DatabaseConnection,
        metrics: &mut crate::core::stage_timing::StageMetrics,
    ) -> Result<Vec<crate::core::canonical_rows::ReceiptTxHashMapRow>> {
        let txn = metrics.time_future("txn_begin", db.begin()).await?;

        metrics
            .time_future(
                "store_block",
                canonical_persistence::flush_near_blocks(
                    &txn,
                    std::slice::from_ref(&projection.block_row),
                ),
            )
            .await
            .context("Failed to store block data")?;

        metrics
            .time_future(
                "store_chunks",
                canonical_persistence::flush_near_chunks(&txn, &projection.chunk_rows),
            )
            .await
            .context("Failed to store chunk data")?;

        metrics
            .time_future(
                "store_transactions",
                canonical_persistence::flush_near_transactions(
                    &txn,
                    &projection.transaction_rows,
                    NearTransactionWriteMode::InsertOnly,
                ),
            )
            .await
            .context("Failed to store transaction data")?;

        metrics
            .time_future(
                "store_receipts",
                canonical_persistence::flush_near_receipts(
                    &txn,
                    &projection.receipt_rows,
                    NearReceiptWriteMode::InsertOnly,
                ),
            )
            .await
            .context("Failed to store receipt data")?;

        metrics
            .time_future(
                "store_execution_outcomes",
                canonical_persistence::flush_near_execution_outcomes(
                    &txn,
                    &projection.execution_outcome_rows,
                ),
            )
            .await
            .context("Failed to store execution outcome data")?;

        metrics
            .time_future(
                "store_receipt_edges",
                canonical_persistence::flush_near_receipt_edges(
                    &txn,
                    &projection.receipt_edge_rows,
                ),
            )
            .await
            .context("Failed to store receipt edge data")?;

        metrics
            .time_future(
                "store_receipt_actions",
                canonical_persistence::flush_near_receipt_actions(
                    &txn,
                    &projection.receipt_action_rows,
                ),
            )
            .await
            .context("Failed to store receipt action data")?;

        let persisted_receipt_tx_rows = metrics
            .time_future(
                "store_receipt_tx_map",
                canonical_persistence::flush_receipt_tx_hash_map_upserts(
                    &txn,
                    &projection.receipt_tx_rows,
                    MODULE_NAME,
                ),
            )
            .await
            .context("Failed to store receipt tx hash map data")?;

        metrics
            .time_future(
                "store_state_changes",
                canonical_persistence::flush_near_state_changes(
                    &txn,
                    &projection.state_change_rows,
                ),
            )
            .await
            .context("Failed to store state change data")?;

        metrics.time_future("txn_commit", txn.commit()).await?;

        Ok(persisted_receipt_tx_rows)
    }

    async fn process_block_internal(
        &self,
        artifacts: &BlockArtifacts,
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()> {
        let facts = artifacts.facts();
        let height = facts.block_height as u64;
        let shard_count = facts.shard_count;
        info!(
            module = MODULE_NAME,
            height = height,
            shard_count = shard_count,
            "Processing NEAR raw block data"
        );

        let mut process_log = BlockProcessingLog::new(MODULE_NAME, height, shard_count);
        process_log
            .metrics_mut()
            .record_count("chunk_count", facts.chunk_count);
        process_log
            .metrics_mut()
            .record_count("transaction_count", facts.transaction_count);
        process_log
            .metrics_mut()
            .record_count("receipt_count", facts.receipt_count);
        process_log.metrics_mut().record_count(
            "receipt_execution_outcome_count",
            facts.receipt_execution_outcome_count,
        );
        process_log
            .metrics_mut()
            .record_count("state_change_count", facts.state_change_count);
        process_log.metrics_mut().record_count(
            "candidate_receipt_ids_before_dedup",
            facts.candidate_receipt_id_count_before_dedup,
        );
        process_log.metrics_mut().record_count(
            "candidate_receipt_ids_after_dedup",
            facts.candidate_receipt_ids.len(),
        );
        process_log
            .metrics_mut()
            .record_count("data_receipt_id_count", facts.data_receipt_ids.len());
        process_log
            .metrics_mut()
            .record_count("receipt_tx_seed_count", facts.receipt_tx_hashes.len());

        let resolved_graph = process_log
            .metrics_mut()
            .time_future(
                "receipt_graph_resolve",
                artifacts.receipt_graph(self.receipt_graph.as_ref(), msg, db),
            )
            .await?;
        process_log
            .metrics_mut()
            .merge_with_prefix("receipt_graph.", resolved_graph.metrics.clone());

        let projection = process_log
            .metrics_mut()
            .time_future(
                "raw_project",
                artifacts.raw_projection(self.receipt_graph.as_ref(), msg, db),
            )
            .await
            .context("Failed to build raw block projection")?;

        let persisted_receipt_tx_rows = self
            .commit_projection(&projection, db, process_log.metrics_mut())
            .await?;

        self.receipt_graph.remember_after_commit(
            facts.block_height,
            projection.new_data_receipt_links,
            &projection.receipt_tx_rows,
            &persisted_receipt_tx_rows,
        );
        process_log
            .metrics_mut()
            .record_count("receipt_tx_hot_cache_size", self.receipt_graph.hot_cache_size());

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

    fn demand(&self) -> ModuleDemand {
        ModuleDemand {
            needs_receipt_graph: true,
            needs_raw_projection: true,
            needs_hos_delta: false,
        }
    }

    #[instrument(skip(self, artifacts, msg, db), fields(module = "raw", height = artifacts.facts().block_height))]
    async fn process_block_with_artifacts(
        &self,
        artifacts: &BlockArtifacts,
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
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
