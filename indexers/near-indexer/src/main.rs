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
    let registry = Arc::new(registry);

    // Check if any modules are enabled
    let enabled_count: usize = registry.enabled_modules().count();
    if enabled_count == 0 {
        info!("No modules enabled, exiting");
        return Ok(());
    }

    info!(enabled_modules = enabled_count, "Modules initialized");

    let (chain, configured_start_height, concurrency) = match &config {
        AppConfig::Indexer(idx) => {
            let n = idx.near.as_ref();
            (
                n.map(|c| c.chain.clone())
                    .unwrap_or_else(|| "mainnet".to_string()),
                n.map(|c| c.start_height).unwrap_or(0),
                n.and_then(|c| c.concurrency).unwrap_or(1),
            )
        }
        _ => ("mainnet".to_string(), 0, 1),
    };

    // Determine the earliest start height needed by any module
    let mut global_start_height = configured_start_height;
    let mut any_used_cursor = false;

    for module in registry.enabled_modules() {
        let (module_start, used_cursor) = near_indexer::core::cursor::choose_module_start_height(
            &db,
            module.name(),
            configured_start_height,
        )
        .await?;

        // If this is the first module or this module needs to start earlier, update global start
        if !any_used_cursor || (used_cursor && module_start < global_start_height) {
            global_start_height = module_start;
        }
        any_used_cursor = any_used_cursor || used_cursor;
    }

    // Configure NEAR Lake stream
    let lake_config = near_indexer::core::lake_stream::LakeStreamConfig {
        chain: chain.clone(),
        start_height: global_start_height,
        concurrency,
    };

    info!(
        chain = %chain,
        start_height = global_start_height,
        resume_source = %if any_used_cursor { "cursor" } else { "config" },
        enabled_modules = enabled_count,
        "Initialized NEAR indexer configuration"
    );

    let (_handle, stream) = near_indexer::core::lake_stream::create_lake_stream(lake_config)?;

    // Start the processing loop
    near_indexer::core::processor::run_processing_loop(registry, db, stream).await?;

    Ok(())
}
