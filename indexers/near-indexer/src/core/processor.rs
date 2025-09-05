use anyhow::Result;
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::ModuleRegistry;

/// Process a single streamer message through all enabled modules with synchronization
pub async fn process_streamer_message(
    registry: &ModuleRegistry,
    db: &DatabaseConnection,
    msg: near_lake_framework::near_indexer_primitives::StreamerMessage,
) -> Result<()> {
    let height = msg.block.header.height as i64;
    let all_module_names = registry.enabled_module_names();

    debug!(
        height,
        modules = all_module_names.len(),
        "Processing block through modules"
    );

    let mut processed_any = false;

    // Process the message through each enabled module that can proceed
    for module in registry.enabled_modules() {
        // Check if this module can process this block (synchronization check)
        let can_process = crate::core::cursor::can_module_process_block(
            db,
            module.name(),
            height,
            &all_module_names,
        )
        .await?;

        if !can_process {
            debug!(
                module = module.name(),
                height, "Module skipping block (waiting for slower modules or already processed)"
            );
            continue;
        }

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

        processed_any = true;
    }

    if processed_any {
        debug!(height, "Block processed through eligible modules");
        Ok(())
    } else {
        // If no modules processed this block, check if we should wait
        // This can happen when faster modules are waiting for slower ones
        debug!(
            height,
            "Block skipped by all modules (synchronization wait)"
        );

        // Check if any module actually needs to process this block
        let mut needs_processing = false;
        for module in registry.enabled_modules() {
            let cursor = module.get_cursor(db).await?;
            if cursor.map(|c| c < height).unwrap_or(true) {
                needs_processing = true;
                break;
            }
        }

        if needs_processing {
            // At least one module needs to process this block but can't due to sync constraints
            // This is expected behavior - we'll retry this block later when dependencies catch up
            debug!(height, "Block waiting for module synchronization");
        } else {
            // All modules have already processed this block - safe to skip
            debug!(height, "Block already processed by all modules");
        }

        Ok(())
    }
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
        info!(
            height = msg.block.header.height,
            "Received block from NEAR Lake stream"
        );
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
