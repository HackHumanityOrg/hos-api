use anyhow::{Context, Result};
use common::config::{AppConfig, ToLog};
use common::telemetry::{self, TelemetrySetup};
use db_core::connection::from_config as db_from_config;
use near_indexer::{modules, ModuleRegistry};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};

#[tokio::main]
async fn main() -> Result<()> {
    let startup_time = Instant::now();

    dotenvy::dotenv().ok();

    let config =
        AppConfig::load_indexer("near-indexer").context("Failed to load indexer config")?;

    let telemetry_setup =
        TelemetrySetup::from_config("near-indexer", config.telemetry(), &config.logging().level)?;
    let _guard = telemetry::init_telemetry(telemetry_setup)?;

    info!(
        config = %config.to_log(),
        version = env!("CARGO_PKG_VERSION"),
        "Starting NEAR indexer"
    );

    let db = Arc::new(
        db_from_config(config.database())
            .await
            .context("DB connection failed")?,
    );

    // Initialize module registry
    let mut registry = ModuleRegistry::new();
    modules::register_all_modules(&mut registry, &config);

    // Initialize all modules
    registry.initialize_all(db.as_ref()).await?;

    // Initialize cursor storage from database
    near_indexer::core::cursor::initialize_cursor_storage(db.as_ref()).await?;
    info!("Cursor storage initialized from database");

    let registry = Arc::new(registry);

    // Check if any modules are enabled
    let enabled_count: usize = registry.enabled_modules().count();
    let enabled_module_names: Vec<&str> = registry.enabled_modules().map(|m| m.name()).collect();

    if enabled_count == 0 {
        warn!("No modules enabled, exiting gracefully");
        return Ok(());
    }

    info!(
        enabled_count = enabled_count,
        enabled_modules = ?enabled_module_names,
        "Modules initialized and ready"
    );

    let (chain, configured_start_height, configured_concurrency, configured_preload_pool_size) =
        match &config {
            AppConfig::Indexer(idx) => {
                let idx = idx.as_ref();
                let n = idx.near.as_ref();
                (
                    n.map(|c| c.chain.clone())
                        .unwrap_or_else(|| "mainnet".to_string()),
                    n.map(|c| c.start_height).unwrap_or(0),
                    n.and_then(|c| c.concurrency).unwrap_or(6), // Default to 6 for parallel processing (conservative with large connection pool)
                    // Default small preload to avoid upstream buffering
                    n.and_then(|c| c.preload_pool_size).unwrap_or(1),
                )
            }
            _ => ("mainnet".to_string(), 0, 6, 1),
        };

    // Find the minimum cursor across all modules to determine stream start point
    let mut stream_start_height = configured_start_height;
    let mut any_module_has_cursor = false;

    for module in registry.enabled_modules() {
        let cursor = near_indexer::core::cursor::get_module_cursor(&db, module.name()).await?;

        if let Some(height) = cursor {
            let next_height = (height as u64).saturating_add(1);
            // Stream needs to start from the earliest position any module needs
            stream_start_height = stream_start_height.min(next_height);
            any_module_has_cursor = true;

            info!(
                module = module.name(),
                cursor = height,
                next_height,
                "Module cursor found"
            );
        } else {
            // Module has no cursor, needs to start from configured start height
            info!(
                module = module.name(),
                "No cursor found, will start from configured height"
            );
        }
    }

    // Configure NEAR Lake stream
    let lake_config = near_indexer::core::lake_stream::LakeStreamConfig {
        chain: chain.clone(),
        start_height: stream_start_height,
        concurrency: 1, // Lake framework concurrency, keep at 1 since we handle parallelism ourselves
        // Keep preload small to limit memory growth from upstream buffering.
        // Use configured value if provided; otherwise defaulted above to 1.
        preload_pool_size: configured_preload_pool_size,
    };

    let startup_elapsed = startup_time.elapsed();
    info!(
        chain = %chain,
        stream_start_height = stream_start_height,
        configured_concurrency = configured_concurrency,
        resume_source = %if any_module_has_cursor { "cursor" } else { "config" },
        enabled_modules = enabled_count,
        startup_ms = startup_elapsed.as_millis(),
        "NEAR indexer startup completed, starting stream processing"
    );

    let (_handle, stream) = near_indexer::core::lake_stream::create_lake_stream(lake_config)?;

    // Start the parallel processing loop
    let processing_result = near_indexer::core::processor::run_parallel_processing(
        registry,
        db.clone(),
        stream,
        configured_concurrency,
    )
    .await;

    match processing_result {
        Ok(()) => {
            info!("NEAR indexer processing completed successfully");
            Ok(())
        }
        Err(e) => {
            error!(error = %e, "NEAR indexer processing failed");
            Err(e)
        }
    }
}
