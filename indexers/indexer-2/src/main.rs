use anyhow::{Context, Result};
use common::config::AppConfig;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Load indexer-specific configuration (type-safe)
    let config = AppConfig::load_indexer("indexer-2").context("Failed to load configuration")?;

    // Initialize tracing
    common::utils::init_tracing(&config.logging().level).context("Failed to initialize tracing")?;

    info!("Starting Indexer 2 Service...");
    info!("Configuration loaded: {:?}", config);

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

    // TODO: Setup indexing pipeline
    // TODO: Start processing loop

    info!("Indexer 2 Service would start processing data");

    Ok(())
}
