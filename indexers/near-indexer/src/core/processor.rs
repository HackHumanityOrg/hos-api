use anyhow::Result;
use futures::{future, StreamExt};
use sea_orm::DatabaseConnection;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{debug, error, info};

use crate::ModuleRegistry;

/// Process a single streamer message through all enabled modules with parallel processing
pub async fn process_streamer_message(
    registry: &ModuleRegistry,
    db: &DatabaseConnection,
    msg: near_lake_framework::near_indexer_primitives::StreamerMessage,
) -> Result<()> {
    let start_time = Instant::now();
    let height = msg.block.header.height as i64;
    let all_module_names = registry.enabled_module_names();

    debug!(
        height,
        modules = all_module_names.len(),
        "Processing block through modules"
    );

    // Collect modules that can process this block
    let mut eligible_modules = Vec::new();
    for module in registry.enabled_modules() {
        // Check if this module can process this block (synchronization check)
        let can_process = crate::core::cursor::can_module_process_block(
            db,
            module.name(),
            height,
            &all_module_names,
        )
        .await?;

        if can_process {
            eligible_modules.push(module);
        } else {
            debug!(
                module = module.name(),
                height, "Module skipping block (waiting for slower modules or already processed)"
            );
        }
    }

    if eligible_modules.is_empty() {
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
            debug!(height, "Block waiting for module synchronization");
        } else {
            debug!(height, "Block already processed by all modules");
        }

        return Ok(());
    }

    // Store count before moving eligible_modules
    let modules_count = eligible_modules.len();
    
    // Process eligible modules in parallel
    let tasks: Vec<_> = eligible_modules
        .into_iter()
        .map(|module| {
            let msg_clone = msg.clone();
            async move {
                let module_name = module.name().to_string();
                
                // Process the block
                if let Err(e) = module.process_block(&msg_clone, db).await {
                    error!(
                        module = module_name,
                        height,
                        error = %e,
                        "Module processing failed"
                    );
                    return Err((module_name.clone(), e));
                }

                // Update cursor after successful processing
                if let Err(e) = module.update_cursor(db, height).await {
                    error!(
                        module = module_name,
                        height,
                        error = %e,
                        "Failed to update module cursor"
                    );
                    return Err((module_name.clone(), e));
                }

                debug!(
                    module = module_name,
                    height, "Module processed successfully"
                );

                Ok(module_name)
            }
        })
        .collect();

    // Wait for all modules to complete
    let results = future::join_all(tasks).await;

    // Check if any modules failed
    for result in results {
        if let Err((module_name, e)) = result {
            error!(
                module = module_name,
                height,
                error = %e,
                "Module failed during parallel processing"
            );
            return Err(e);
        }
    }

    let processing_time = start_time.elapsed();
    info!(
        height,
        processing_time_ms = processing_time.as_millis(),
        modules_processed = modules_count,
        "Block processed successfully through all eligible modules"
    );
    Ok(())
}

/// Process a single block with retry logic
async fn process_block_with_retry(
    registry: Arc<ModuleRegistry>,
    db: DatabaseConnection,
    msg: near_lake_framework::near_indexer_primitives::StreamerMessage,
) -> Result<()> {
    let height = msg.block.header.height;
    let base_backoff_ms: u64 = 250;
    let max_backoff_ms: u64 = 30_000;
    let mut backoff_ms: u64 = base_backoff_ms;

    debug!(height, "Processing block with retry logic");

    loop {
        match process_streamer_message(&registry, &db, msg.clone()).await {
            Ok(_) => {
                debug!(height, "Block processed successfully");
                return Ok(());
            }
            Err(e) => {
                error!(
                    error = %e,
                    backoff_ms,
                    height,
                    "Block processing failed, backing off and retrying"
                );

                // Compute jitter using current time nanos
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                let half = backoff_ms / 2;
                let jitter_range = if half > 0 { half } else { 1 };
                let jitter = (now.subsec_nanos() as u64) % jitter_range;
                let sleep_ms = half + jitter;

                tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                backoff_ms = (backoff_ms.saturating_mul(2)).min(max_backoff_ms);
                // Continue loop to retry
            }
        }
    }
}

/// Main concurrent processing loop with configurable parallelism
pub async fn run_processing_loop(
    registry: Arc<ModuleRegistry>,
    db: DatabaseConnection,
    stream: tokio::sync::mpsc::Receiver<
        near_lake_framework::near_indexer_primitives::StreamerMessage,
    >,
    concurrency: usize,
) -> Result<()> {
    info!(
        concurrency = concurrency,
        "Starting NEAR indexer concurrent processing loop"
    );

    // Metrics for throughput tracking
    let mut blocks_processed = 0u64;
    let mut last_stats_time = Instant::now();
    let stats_interval = Duration::from_secs(30); // Log stats every 30 seconds

    // Convert receiver to stream for concurrent processing
    let stream = ReceiverStream::new(stream);

    // Process blocks concurrently using buffer_unordered
    let mut concurrent_stream = stream
        .map(|msg| {
            let registry = registry.clone();
            let db = db.clone();
            info!(
                height = msg.block.header.height,
                "Received block from NEAR Lake stream"
            );
            
            // Process block with retry logic
            process_block_with_retry(registry, db, msg)
        })
        .buffer_unordered(concurrency);

    // Process the concurrent results
    while let Some(result) = concurrent_stream.next().await {
        match result {
            Ok(_) => {
                blocks_processed += 1;
                
                // Log throughput statistics periodically
                let elapsed = last_stats_time.elapsed();
                if elapsed >= stats_interval {
                    let blocks_per_second = blocks_processed as f64 / elapsed.as_secs_f64();
                    info!(
                        blocks_processed = blocks_processed,
                        elapsed_secs = elapsed.as_secs(),
                        blocks_per_second = blocks_per_second,
                        concurrency = concurrency,
                        "Processing throughput statistics"
                    );
                    
                    // Reset counters
                    blocks_processed = 0;
                    last_stats_time = Instant::now();
                }
            }
            Err(e) => {
                // This should not happen as retry logic is inside process_block_with_retry
                // But if it does, log and continue with other blocks
                error!(error = %e, "Unexpected error in concurrent processing");
                
                // In a production system, you might want to implement more sophisticated
                // error handling here, such as circuit breakers or alerting
            }
        }
    }

    info!("NEAR indexer concurrent processing loop ended");
    Ok(())
}
