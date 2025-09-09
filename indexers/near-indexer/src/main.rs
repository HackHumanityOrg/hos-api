use anyhow::{Context, Result};
use common::config::{AppConfig, ToLog};
use common::telemetry::{self, TelemetrySetup};
use db_core::connection::from_config as db_from_config;
use near_indexer::{modules, ModuleRegistry};
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let config =
        AppConfig::load_indexer("near-indexer").context("Failed to load indexer config")?;

    let telemetry_setup =
        TelemetrySetup::from_config("near-indexer", config.telemetry(), &config.logging().level)?;
    let _guard = telemetry::init_telemetry(telemetry_setup)?;

    info!(config = %config.to_log(), "Starting NEAR indexer");

    let db = db_from_config(config.database())
        .await
        .context("DB connection failed")?;

    // Initialize module registry
    let mut registry = ModuleRegistry::new();
    modules::register_all_modules(&mut registry, &config);

    // Initialize all modules
    registry.initialize_all(&db).await?;

    // Initialize cursor storage from database
    near_indexer::core::cursor::initialize_cursor_storage(&db).await?;
    info!("Cursor storage initialized from database");

    let registry = Arc::new(registry);

    // Check if any modules are enabled
    let enabled_count: usize = registry.enabled_modules().count();
    if enabled_count == 0 {
        info!("No modules enabled, exiting");
        return Ok(());
    }

    info!(enabled_modules = enabled_count, "Modules initialized");

    let (chain, configured_start_height, configured_concurrency) = match &config {
        AppConfig::Indexer(idx) => {
            let n = idx.near.as_ref();
            (
                n.map(|c| c.chain.clone())
                    .unwrap_or_else(|| "mainnet".to_string()),
                n.map(|c| c.start_height).unwrap_or(0),
                n.and_then(|c| c.concurrency).unwrap_or(6), // Default to 6 for parallel processing (conservative with large connection pool)
            )
        }
        _ => ("mainnet".to_string(), 0, 6),
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
    };

    info!(
        chain = %chain,
        stream_start_height,
        configured_concurrency,
        resume_source = %if any_module_has_cursor { "cursor" } else { "config" },
        enabled_modules = enabled_count,
        "Starting NEAR parallel indexer"
    );

    let (_handle, stream) = near_indexer::core::lake_stream::create_lake_stream(lake_config)?;

    // Start the parallel processing loop
    near_indexer::core::processor::run_parallel_processing(
        registry,
        db,
        stream,
        configured_concurrency,
    )
    .await?;

    Ok(())
}
