use anyhow::Result;
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::ModuleRegistry;

/// Process a single streamer message through all enabled modules
pub async fn process_streamer_message(
    registry: &ModuleRegistry,
    db: &DatabaseConnection,
    msg: near_lake_framework::near_indexer_primitives::StreamerMessage,
) -> Result<()> {
    let height = msg.block.header.height as i64;

    debug!(height, "Processing block through modules");

    // Process the message through each enabled module
    for module in registry.enabled_modules() {
        if let Err(e) = module.process_block(&msg, db).await {
            error!(
                module = module.name(),
                height,
                error = %e,
                "Module processing failed"
            );
            return Err(e);
        }

        // Update cursor after successful processing
        if let Err(e) = module.update_cursor(db, height).await {
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

/// Main processing loop with error handling and backoff
pub async fn run_processing_loop(
    registry: Arc<ModuleRegistry>,
    db: DatabaseConnection,
    mut stream: tokio::sync::mpsc::Receiver<
        near_lake_framework::near_indexer_primitives::StreamerMessage,
    >,
) -> Result<()> {
    // Exponential backoff with jitter on errors
    let base_backoff_ms: u64 = 250;
    let max_backoff_ms: u64 = 30_000;
    let mut backoff_ms: u64 = base_backoff_ms;

    info!("Starting NEAR indexer processing loop");

    while let Some(msg) = stream.recv().await {
        // Retry the same block on failure to avoid skipping it
        loop {
            match process_streamer_message(&registry, &db, msg.clone()).await {
                Ok(_) => {
                    // Reset backoff after success
                    backoff_ms = base_backoff_ms;
                    break;
                }
                Err(e) => {
                    error!(
                        error = %e,
                        backoff_ms,
                        height = msg.block.header.height,
                        "Failed processing block; backing off and retrying"
                    );

                    // Compute a simple jitter using current time nanos
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default();
                    let half = backoff_ms / 2;
                    let jitter_range = if half > 0 { half } else { 1 };
                    let jitter = (now.subsec_nanos() as u64) % jitter_range;
                    let sleep_ms = half + jitter;

                    tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                    backoff_ms = (backoff_ms.saturating_mul(2)).min(max_backoff_ms);
                    // loop and retry the same message
                }
            }
        }
    }

    info!("NEAR indexer processing loop ended");
    Ok(())
}
