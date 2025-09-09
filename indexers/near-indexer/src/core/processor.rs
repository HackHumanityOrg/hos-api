use anyhow::Result;
use futures::{StreamExt, TryStreamExt};
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info};

use crate::ModuleRegistry;

/// Process a single block through all enabled modules independently  
async fn process_block_independent(
    registry: Arc<ModuleRegistry>,
    db: DatabaseConnection,
    msg: near_lake_framework::near_indexer_primitives::StreamerMessage,
) -> Result<()> {
    let height = msg.block.header.height as i64;

    debug!(height, "Processing block through all modules");

    // Process all modules independently - no synchronization
    for module in registry.enabled_modules() {
        // Check if this module needs this block
        let cursor = module.get_cursor(&db).await?;
        if cursor.map(|c| c >= height).unwrap_or(false) {
            debug!(
                module = module.name(),
                height, cursor, "Module already processed this block, skipping"
            );
            continue;
        }

        debug!(module = module.name(), height, "Processing block in module");

        // Process block and update cursor
        if let Err(e) = module.process_block(&msg, &db).await {
            error!(
                module = module.name(),
                height,
                error = %e,
                "Module processing failed"
            );
            return Err(e);
        }

        if let Err(e) = module.update_cursor(&db, height).await {
            error!(
                module = module.name(),
                height,
                error = %e,
                "Failed to update module cursor"
            );
            return Err(e);
        }

        debug!(
            module = module.name(),
            height, "Module processed successfully"
        );
    }

    debug!(height, "Block processed through all modules");
    Ok(())
}

/// Parallel processing loop using buffer_unordered for concurrent block processing
pub async fn run_parallel_processing(
    registry: Arc<ModuleRegistry>,
    db: DatabaseConnection,
    stream: tokio::sync::mpsc::Receiver<
        near_lake_framework::near_indexer_primitives::StreamerMessage,
    >,
    concurrency: usize,
) -> Result<()> {
    info!(
        concurrency,
        "Starting NEAR parallel indexer processing with aggressive connection pool settings"
    );

    ReceiverStream::new(stream)
        .map(|msg| {
            let registry = registry.clone();
            let db = db.clone();

            async move {
                let height = msg.block.header.height;
                info!(height, "Processing block");

                // Process block with exponential backoff retry
                let mut backoff_ms: u64 = 250;
                let max_backoff_ms: u64 = 30_000;

                loop {
                    match process_block_independent(registry.clone(), db.clone(), msg.clone()).await
                    {
                        Ok(_) => {
                            info!(height, "Block processed successfully");
                            return Ok::<_, anyhow::Error>(());
                        }
                        Err(e) => {
                            let is_pool_error = e.to_string().contains("Connection pool timed out")
                                || e.to_string()
                                    .contains("Failed to acquire connection from pool");

                            error!(
                                error = %e,
                                height,
                                backoff_ms,
                                is_pool_error,
                                "Block processing failed, retrying with backoff"
                            );

                            // For pool errors, add extra delay to allow connections to be recycled
                            if is_pool_error {
                                backoff_ms = backoff_ms.max(5000); // Minimum 5 second delay for pool errors
                                error!(height, "Connection pool error - using extended backoff");
                            }

                            // Exponential backoff with jitter
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default();
                            let jitter = (now.subsec_nanos() as u64) % (backoff_ms / 2 + 1);
                            let sleep_ms = backoff_ms / 2 + jitter;

                            tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                            backoff_ms = (backoff_ms.saturating_mul(2)).min(max_backoff_ms);
                        }
                    }
                }
            }
        })
        .buffer_unordered(concurrency)
        .try_for_each(|_| async { Ok(()) })
        .await?;

    info!("NEAR parallel indexer processing ended");
    Ok(())
}
