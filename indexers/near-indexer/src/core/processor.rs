use anyhow::Result;
use futures::{StreamExt, TryStreamExt};
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use std::time::Instant;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info, instrument, warn};

use crate::ModuleRegistry;

/// Process a single block through all enabled modules independently  
#[instrument(skip(registry, db, msg), fields(height = %msg.block.header.height))]
async fn process_block_independent(
    registry: Arc<ModuleRegistry>,
    db: DatabaseConnection,
    msg: near_lake_framework::near_indexer_primitives::StreamerMessage,
) -> Result<()> {
    let height = msg.block.header.height as i64;
    let start_time = Instant::now();

    debug!(height = height, "Processing block through all modules");

    // Process all modules independently - no synchronization
    let mut modules_processed = 0;
    for module in registry.enabled_modules() {
        let module_start = Instant::now();

        // Check if this module needs this block
        let cursor = module.get_cursor(&db).await?;
        if cursor.map(|c| c >= height).unwrap_or(false) {
            debug!(
                module = module.name(),
                height = height,
                cursor = cursor,
                "Module already processed this block, skipping"
            );
            continue;
        }

        debug!(
            module = module.name(),
            height = height,
            "Processing block in module"
        );

        // Process block and update cursor
        if let Err(e) = module.process_block(&msg, &db).await {
            error!(
                module = module.name(),
                height = height,
                error = %e,
                "Module processing failed"
            );
            return Err(e);
        }

        if let Err(e) = module.update_cursor(&db, height).await {
            error!(
                module = module.name(),
                height = height,
                error = %e,
                "Failed to update module cursor"
            );
            return Err(e);
        }

        let module_elapsed = module_start.elapsed();
        debug!(
            module = module.name(),
            height = height,
            elapsed_ms = module_elapsed.as_millis(),
            "Module processed successfully"
        );

        modules_processed += 1;
    }

    let total_elapsed = start_time.elapsed();
    debug!(
        height = height,
        modules_processed = modules_processed,
        elapsed_ms = total_elapsed.as_millis(),
        "Block processed through all modules"
    );
    Ok(())
}

/// Parallel processing loop using buffer_unordered for concurrent block processing
#[instrument(skip(registry, db, stream), fields(concurrency = %concurrency))]
pub async fn run_parallel_processing(
    registry: Arc<ModuleRegistry>,
    db: DatabaseConnection,
    stream: tokio::sync::mpsc::Receiver<
        near_lake_framework::near_indexer_primitives::StreamerMessage,
    >,
    concurrency: usize,
) -> Result<()> {
    info!(
        concurrency = concurrency,
        "Starting NEAR parallel indexer processing"
    );

    ReceiverStream::new(stream)
        .map(|msg| {
            let registry = registry.clone();
            let db = db.clone();

            async move {
                let height = msg.block.header.height;
                let block_start = Instant::now();
                info!(height = height, "Processing block");

                // Process block with exponential backoff retry
                let mut backoff_ms: u64 = 250;
                let max_backoff_ms: u64 = 30_000;
                let mut attempt = 1;

                loop {
                    match process_block_independent(registry.clone(), db.clone(), msg.clone()).await
                    {
                        Ok(_) => {
                            let elapsed = block_start.elapsed();
                            if attempt > 1 {
                                info!(
                                    height = height,
                                    attempt = attempt,
                                    elapsed_ms = elapsed.as_millis(),
                                    "Block processed successfully after retries"
                                );
                            } else {
                                info!(
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
        .buffer_unordered(concurrency)
        .try_for_each(|_| async { Ok(()) })
        .await?;

    info!("NEAR parallel indexer processing completed");
    Ok(())
}
