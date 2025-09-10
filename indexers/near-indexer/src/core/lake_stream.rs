use anyhow::{Context, Result};
use near_lake_framework::LakeConfigBuilder;
use tracing::{info, warn};

/// Configuration for the NEAR Lake stream
#[derive(Debug, Clone)]
pub struct LakeStreamConfig {
    pub chain: String,
    pub start_height: u64,
    pub concurrency: usize,
}

/// Type alias for the complex return type of the Lake stream
type LakeStreamResult = (
    tokio::task::JoinHandle<Result<(), anyhow::Error>>,
    tokio::sync::mpsc::Receiver<near_lake_framework::near_indexer_primitives::StreamerMessage>,
);

/// Initialize a NEAR Lake stream with the given configuration
pub fn create_lake_stream(config: LakeStreamConfig) -> Result<LakeStreamResult> {
    let mut lake_cfg = LakeConfigBuilder::default().start_block_height(config.start_height);

    match config.chain.as_str() {
        "mainnet" => lake_cfg = lake_cfg.mainnet(),
        "testnet" => lake_cfg = lake_cfg.testnet(),
        other => {
            warn!(chain = %other, "Unknown chain id, defaulting to mainnet");
            lake_cfg = lake_cfg.mainnet();
        }
    }

    info!(
        chain = %config.chain,
        start_height = config.start_height,
        concurrency = config.concurrency,
        "Initializing NEAR Lake stream"
    );

    let lake_config = lake_cfg.build().context("Failed to build Lake config")?;

    info!(
        start_height = config.start_height,
        "Lake config built successfully, creating streamer"
    );

    let (handle, stream) = near_lake_framework::streamer(lake_config);

    info!("NEAR Lake streamer created successfully");

    Ok((handle, stream))
}
