use anyhow::Result;
use futures::{StreamExt, TryStreamExt};
use sea_orm::DatabaseConnection;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info, instrument, warn};

use crate::core::prepared_block::PreparedBlockContext;
use crate::core::stage_timing::StageMetrics;
use crate::ModuleRegistry;

fn module_is_assigned(assigned_modules: &HashSet<String>, module_name: &str) -> bool {
    assigned_modules.contains(module_name)
}

/// Process a single block through all enabled modules independently  
#[instrument(skip(registry, db, msg), fields(height = %msg.block.header.height))]
async fn process_block_independent(
    registry: Arc<ModuleRegistry>,
    db: Arc<DatabaseConnection>,
    msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
    assigned_modules: &HashSet<String>,
    stream_id: &str,
) -> Result<()> {
    let height = msg.block.header.height as i64;
    let prev_height = msg.block.header.prev_height.map(|value| value as i64);
    let start_time = Instant::now();
    let mut block_metrics = StageMetrics::new();
    let mut assigned_module_count = 0usize;
    let mut skipped_module_count = 0usize;

    debug!(
        stream_id = stream_id,
        height = height,
        "Processing block through assigned modules"
    );

    let mut modules_to_process = Vec::new();
    let cursor_check_start = Instant::now();
    for module in registry
        .enabled_modules()
        .filter(|module| module_is_assigned(assigned_modules, module.name()))
    {
        assigned_module_count += 1;

        // Check if this module needs this block
        let cursor = module.get_cursor(db.as_ref()).await?;
        if cursor.map(|c| c >= height).unwrap_or(false) {
            skipped_module_count += 1;
            debug!(
                stream_id = stream_id,
                module = module.name(),
                height = height,
                cursor = cursor,
                "Module already processed this block, skipping"
            );
            continue;
        }

        debug!(
            stream_id = stream_id,
            module = module.name(),
            height = height,
            "Module scheduled for block processing"
        );

        modules_to_process.push(module);
    }
    block_metrics.record_phase_elapsed("cursor_check", cursor_check_start);
    block_metrics.record_count("assigned_module_count", assigned_module_count);
    block_metrics.record_count("skipped_module_count", skipped_module_count);

    if modules_to_process.is_empty() {
        debug!(
            stream_id = stream_id,
            height = height,
            "All assigned modules already processed this block"
        );
        return Ok(());
    }

    let prepare_block_start = Instant::now();
    let prepared_build = PreparedBlockContext::from_message_with_metrics(msg)?;
    block_metrics.record_phase_elapsed("prepare_block", prepare_block_start);
    block_metrics.merge_with_prefix("prepare.", prepared_build.metrics);
    let prepared = prepared_build.context;

    let mut modules_processed = 0;
    for module in modules_to_process {
        let module_process_start = Instant::now();
        if let Err(e) = module
            .process_prepared_block(&prepared, msg, db.as_ref())
            .await
        {
            error!(
                stream_id = stream_id,
                module = module.name(),
                height = height,
                error = %e,
                "Module processing failed"
            );
            return Err(e);
        }
        block_metrics.record_phase_elapsed(
            format!("module.{}.process", module.name()),
            module_process_start,
        );

        let cursor_update_start = Instant::now();
        if let Err(e) = module.update_cursor(db.as_ref(), height, prev_height).await {
            error!(
                stream_id = stream_id,
                module = module.name(),
                height = height,
                error = %e,
                "Failed to update module cursor"
            );
            return Err(e);
        }
        block_metrics.record_phase_elapsed(
            format!("module.{}.cursor_update", module.name()),
            cursor_update_start,
        );

        debug!(
            stream_id = stream_id,
            module = module.name(),
            height = height,
            elapsed_ms = module_process_start.elapsed().as_millis(),
            "Module processed successfully"
        );

        modules_processed += 1;
    }

    let total_elapsed = start_time.elapsed();
    block_metrics.record_count("modules_processed", modules_processed);
    info!(
        stream_id = stream_id,
        height = height,
        modules_processed = modules_processed,
        elapsed_ms = total_elapsed.as_millis(),
        phase_ms = ?block_metrics.phase_ms(),
        counts = ?block_metrics.counts(),
        flags = ?block_metrics.flags(),
        "Block processed through assigned modules"
    );
    Ok(())
}

/// Parallel processing loop using buffered for concurrent block processing
#[instrument(skip(registry, db, stream, assigned_modules), fields(concurrency = %concurrency, stream_id = %stream_id))]
pub async fn run_parallel_processing(
    registry: Arc<ModuleRegistry>,
    db: Arc<DatabaseConnection>,
    stream: tokio::sync::mpsc::Receiver<
        near_lake_framework::near_indexer_primitives::StreamerMessage,
    >,
    concurrency: usize,
    assigned_modules: Arc<HashSet<String>>,
    stream_id: String,
) -> Result<()> {
    let mut assigned_list: Vec<String> = assigned_modules.iter().cloned().collect();
    assigned_list.sort();

    info!(
        stream_id = %stream_id,
        concurrency = concurrency,
        assigned_modules = ?assigned_list,
        "Starting NEAR parallel indexer processing"
    );

    ReceiverStream::new(stream)
        .map(|msg| {
            let registry = registry.clone();
            let db = db.clone();
            let assigned_modules = assigned_modules.clone();
            let stream_id = stream_id.clone();

            async move {
                let height = msg.block.header.height;
                let block_start = Instant::now();
                info!(stream_id = %stream_id, height = height, "Processing block");

                // Process block with exponential backoff retry
                let mut backoff_ms: u64 = 250;
                let max_backoff_ms: u64 = 30_000;
                let mut attempt = 1;

                loop {
                    match process_block_independent(
                        registry.clone(),
                        db.clone(),
                        &msg,
                        assigned_modules.as_ref(),
                        &stream_id,
                    )
                    .await
                    {
                        Ok(_) => {
                            let elapsed = block_start.elapsed();
                            if attempt > 1 {
                                info!(
                                    stream_id = %stream_id,
                                    height = height,
                                    attempt = attempt,
                                    elapsed_ms = elapsed.as_millis(),
                                    "Block processed successfully after retries"
                                );
                            } else {
                                debug!(
                                    stream_id = %stream_id,
                                    height = height,
                                    elapsed_ms = elapsed.as_millis(),
                                    "Block processed successfully"
                                );
                            }
                            return Ok::<_, anyhow::Error>(());
                        }
                        Err(e) => {
                            let is_pool_error = e.to_string().contains("Connection pool timed out")
                                || e.to_string()
                                    .contains("Failed to acquire connection from pool");

                            // Only log first few errors or pool errors to avoid spam
                            if attempt <= 3 || is_pool_error {
                                error!(
                                    stream_id = %stream_id,
                                    error = %e,
                                    height = height,
                                    attempt = attempt,
                                    backoff_ms = backoff_ms,
                                    is_pool_error = is_pool_error,
                                    "Block processing failed, retrying"
                                );
                            } else if attempt % 10 == 0 {
                                // Log every 10th attempt to avoid spam but show we're still trying
                                warn!(
                                    stream_id = %stream_id,
                                    height = height,
                                    attempt = attempt,
                                    backoff_ms = backoff_ms,
                                    "Still retrying block processing (logged every 10 attempts)"
                                );
                            }

                            // For pool errors, add extra delay to allow connections to be recycled
                            if is_pool_error {
                                backoff_ms = backoff_ms.max(5000); // Minimum 5 second delay for pool errors
                            }

                            // Exponential backoff with jitter
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default();
                            let jitter = (now.subsec_nanos() as u64) % (backoff_ms / 2 + 1);
                            let sleep_ms = backoff_ms / 2 + jitter;

                            tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                            backoff_ms = (backoff_ms.saturating_mul(2)).min(max_backoff_ms);
                            attempt += 1;
                        }
                    }
                }
            }
        })
        .buffered(concurrency)
        .try_for_each(|_| async { Ok(()) })
        .await?;

    info!(stream_id = %stream_id, "NEAR parallel indexer processing completed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_assignment_matches_exact_name() {
        let assigned_modules =
            HashSet::from(["tokens".to_string(), "venear_governance".to_string()]);

        assert!(module_is_assigned(&assigned_modules, "tokens"));
        assert!(module_is_assigned(&assigned_modules, "venear_governance"));
        assert!(!module_is_assigned(&assigned_modules, "accounts"));
    }
}
