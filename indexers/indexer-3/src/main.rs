use anyhow::{Context, Result};
use common::config::AppConfig;
use common::telemetry::{self, TelemetrySetup};
use opentelemetry::KeyValue;
use tracing::{info, instrument};

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Load indexer-specific configuration (type-safe)
    let config = AppConfig::load_indexer("indexer-3").context("Failed to load configuration")?;

    // Initialize telemetry (Dash0 if configured, basic tracing otherwise)
    let telemetry_setup =
        TelemetrySetup::from_config("indexer-3", config.telemetry(), &config.logging().level)
            .context("Failed to create telemetry setup")?;

    let _telemetry_guard =
        telemetry::init_telemetry(telemetry_setup).context("Failed to initialize telemetry")?;

    info!("Starting Indexer 3 Service...");
    info!("Configuration loaded: {:?}", config);

    // Record indexer startup metric
    if config.telemetry().enabled {
        telemetry::increment_counter(
            "indexer.startup",
            &[
                KeyValue::new("service", "indexer-3"),
                KeyValue::new("type", "auxiliary"),
            ],
        );
    }

    // Initialize database connection
    let db_result = db_core::connection::from_config(config.database()).await;

    match &db_result {
        Ok(_) => {
            info!("Database connection established successfully");
        }
        Err(e) => {
            tracing::warn!("Failed to connect to database: {}", e);
            info!("Starting indexer without database connection - will retry when needed");
        }
    }

    // Setup indexing pipeline
    run_indexing_loop(&config).await?;

    Ok(())
}

#[instrument(skip(config))]
async fn run_indexing_loop(config: &AppConfig) -> Result<()> {
    info!("Starting indexing loop");

    // Simulate indexing work
    loop {
        // Record processing metrics
        let start = std::time::Instant::now();

        // Simulate processing
        process_batch().await?;

        let duration = start.elapsed().as_millis() as f64;

        // Record metrics if telemetry is enabled
        if config.telemetry().enabled {
            telemetry::duration_histogram("indexer.batch.duration")
                .record(duration, &[KeyValue::new("indexer", "indexer-3")]);

            telemetry::increment_counter(
                "indexer.batches.processed",
                &[
                    KeyValue::new("indexer", "indexer-3"),
                    KeyValue::new("status", "success"),
                ],
            );
        }

        // Sleep before next batch
        tokio::time::sleep(tokio::time::Duration::from_secs(20)).await;
    }
}

#[instrument]
async fn process_batch() -> Result<()> {
    tracing::debug!("Processing batch");

    // Simulate some work
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    tracing::debug!("Batch processed successfully");
    Ok(())
}
