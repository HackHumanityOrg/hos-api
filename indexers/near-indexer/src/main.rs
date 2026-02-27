use anyhow::{Context, Result};
use common::config::{AppConfig, ToLog};
use common::telemetry::{self, TelemetrySetup};
use db_core::connection::from_config as db_from_config;
use near_indexer::{modules, ModuleRegistry};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};

fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let config =
        AppConfig::load_indexer("near-indexer").context("Failed to load indexer config")?;

    let telemetry_setup = TelemetrySetup::from_config(
        "near-indexer",
        config.telemetry(),
        &config.logging().level,
        config.logging().format.as_deref(),
    )?;
    let _guard = telemetry::init_telemetry(telemetry_setup)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Failed to create Tokio runtime")?;

    if let Err(err) = runtime.block_on(run(config)) {
        sentry::integrations::anyhow::capture_anyhow(&err);
        return Err(err);
    }

    Ok(())
}

async fn run(config: AppConfig) -> Result<()> {
    let startup_time = Instant::now();

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

    // Find stream start point from per-module starts:
    // - modules with cursor resume from cursor + 1
    // - modules without cursor start from configured_start_height
    let mut stream_start_height: Option<u64> = None;
    let mut any_module_has_cursor = false;

    for module in registry.enabled_modules() {
        let cursor = near_indexer::core::cursor::get_module_cursor(&db, module.name()).await?;

        let module_start_height = if let Some(height) = cursor {
            let next_height = (height as u64).saturating_add(1);
            any_module_has_cursor = true;

            info!(
                module = module.name(),
                cursor = height,
                next_height,
                "Module cursor found"
            );

            next_height
        } else {
            if configured_start_height > 0 {
                // Seed in-memory cursor so out-of-order block completion still advances
                // contiguous cursors correctly from the configured start.
                let seed_height =
                    i64::try_from(configured_start_height.saturating_sub(1)).unwrap_or(i64::MAX);
                near_indexer::core::cursor::seed_module_cursor(module.name(), seed_height);
            }

            // Module has no cursor, needs to start from configured start height
            info!(
                module = module.name(),
                configured_start_height, "No cursor found, will start from configured height"
            );

            configured_start_height
        };

        stream_start_height = Some(
            stream_start_height
                .map(|current| current.min(module_start_height))
                .unwrap_or(module_start_height),
        );
    }

    let stream_start_height = stream_start_height.unwrap_or(configured_start_height);

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

#[cfg(test)]
mod tests {
    fn resolve_stream_start_height_for_test(
        configured_start_height: u64,
        module_cursors: &[Option<i64>],
    ) -> u64 {
        let mut stream_start_height: Option<u64> = None;

        for cursor in module_cursors {
            let module_start_height = if let Some(height) = cursor {
                let non_negative = (*height).max(0) as u64;
                non_negative.saturating_add(1)
            } else {
                configured_start_height
            };

            stream_start_height = Some(
                stream_start_height
                    .map(|current| current.min(module_start_height))
                    .unwrap_or(module_start_height),
            );
        }

        stream_start_height.unwrap_or(configured_start_height)
    }

    #[test]
    fn stream_start_uses_min_cursor_plus_one_when_all_modules_resuming() {
        let start = resolve_stream_start_height_for_test(0, &[Some(120), Some(95), Some(300)]);
        assert_eq!(start, 96);
    }

    #[test]
    fn stream_start_uses_config_for_modules_without_cursor() {
        let start = resolve_stream_start_height_for_test(500, &[Some(900), None, Some(800)]);
        assert_eq!(start, 500);
    }
}
