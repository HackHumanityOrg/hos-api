use anyhow::Result;
use futures::{StreamExt, TryStreamExt};
use sea_orm::DatabaseConnection;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info, instrument, warn};

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

    debug!(
        stream_id = stream_id,
        height = height,
        "Processing block through assigned modules"
    );

    // Process all modules independently - no synchronization
    let mut modules_processed = 0;
    for module in registry
        .enabled_modules()
        .filter(|module| module_is_assigned(assigned_modules, module.name()))
    {
        let module_start = Instant::now();

        // Check if this module needs this block
        let cursor = module.get_cursor(db.as_ref()).await?;
        if cursor.map(|c| c >= height).unwrap_or(false) {
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
            "Processing block in module"
        );

        // Process block and update cursor
        if let Err(e) = module.process_block(msg, db.as_ref()).await {
            error!(
                stream_id = stream_id,
                module = module.name(),
                height = height,
                error = %e,
                "Module processing failed"
            );
            return Err(e);
        }

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

        let module_elapsed = module_start.elapsed();
        debug!(
            stream_id = stream_id,
            module = module.name(),
            height = height,
            elapsed_ms = module_elapsed.as_millis(),
            "Module processed successfully"
        );

        modules_processed += 1;
    }

    let total_elapsed = start_time.elapsed();
    debug!(
        stream_id = stream_id,
        height = height,
        modules_processed = modules_processed,
        elapsed_ms = total_elapsed.as_millis(),
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
                                info!(
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
