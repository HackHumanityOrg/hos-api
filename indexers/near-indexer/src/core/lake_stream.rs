use anyhow::{anyhow, Context, Result};
use near_lake_framework::{FastNearConfigBuilder, LakeConfigBuilder};
use tracing::{info, warn};

const FASTNEAR_MAINNET_FIRST_BLOCK: u64 = 9_820_210;
const FASTNEAR_TESTNET_FIRST_BLOCK: u64 = 42_376_888;
const DEFAULT_FASTNEAR_NUM_THREADS: u64 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NearChain {
    Mainnet,
    Testnet,
}

impl NearChain {
    fn parse(chain: &str) -> Result<Self> {
        match chain {
            "mainnet" => Ok(Self::Mainnet),
            "testnet" => Ok(Self::Testnet),
            other => Err(anyhow!(
                "Unsupported NEAR chain '{other}'. Expected 'mainnet' or 'testnet'"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Mainnet => "mainnet",
            Self::Testnet => "testnet",
        }
    }

    fn fastnear_first_block_height(self) -> u64 {
        match self {
            Self::Mainnet => FASTNEAR_MAINNET_FIRST_BLOCK,
            Self::Testnet => FASTNEAR_TESTNET_FIRST_BLOCK,
        }
    }
}

fn normalize_fastnear_start_height(chain: NearChain, requested_start_height: u64) -> u64 {
    requested_start_height.max(chain.fastnear_first_block_height())
}

/// Configuration for the NEAR Lake stream
#[derive(Debug, Clone)]
pub struct LakeStreamConfig {
    pub chain: String,
    pub start_height: u64,
    pub concurrency: usize,
    pub preload_pool_size: usize,
}

/// Type alias for the complex return type of the Lake stream
type LakeStreamResult = (
    tokio::task::JoinHandle<Result<(), anyhow::Error>>,
    tokio::sync::mpsc::Receiver<near_lake_framework::near_indexer_primitives::StreamerMessage>,
);

/// Initialize a NEAR Lake stream with the given configuration
pub fn create_lake_stream(config: LakeStreamConfig) -> Result<LakeStreamResult> {
    let chain = NearChain::parse(config.chain.as_str())?;

    let mut lake_cfg = LakeConfigBuilder::default()
        .start_block_height(config.start_height)
        .blocks_preload_pool_size(config.preload_pool_size);

    lake_cfg = match chain {
        NearChain::Mainnet => lake_cfg.mainnet(),
        NearChain::Testnet => lake_cfg.testnet(),
    };

    info!(
        chain = chain.as_str(),
        start_height = config.start_height,
        concurrency = config.concurrency,
        preload_pool_size = config.preload_pool_size,
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

/// Initialize a FastNEAR (neardata.xyz) stream with the given configuration.
/// Unlike the S3-based stream, this does not require AWS credentials and supports
/// an optional Bearer token for authenticated access.
pub fn create_fastnear_stream(
    config: LakeStreamConfig,
    api_key: Option<String>,
    num_threads: Option<u64>,
) -> Result<LakeStreamResult> {
    let chain = NearChain::parse(config.chain.as_str())?;
    let start_height = normalize_fastnear_start_height(chain, config.start_height);
    if start_height != config.start_height {
        warn!(
            chain = chain.as_str(),
            configured_start_height = config.start_height,
            adjusted_start_height = start_height,
            "FastNEAR start height is below first available block; adjusting"
        );
    }

    let effective_num_threads = num_threads.unwrap_or(DEFAULT_FASTNEAR_NUM_THREADS);
    let effective_num_threads = if effective_num_threads == 0 {
        warn!(
            configured_num_threads = effective_num_threads,
            fallback_num_threads = 1,
            "Invalid FastNEAR num_threads value, falling back to 1"
        );
        1
    } else {
        effective_num_threads
    };

    let mut builder = FastNearConfigBuilder::default()
        .start_block_height(start_height)
        .num_threads(effective_num_threads)
        .blocks_preload_pool_size(config.preload_pool_size);

    builder = match chain {
        NearChain::Mainnet => builder.mainnet(),
        NearChain::Testnet => builder.testnet(),
    };

    if let Some(key) = api_key {
        builder = builder.authorization_token(Some(key));
    }

    info!(
        chain = chain.as_str(),
        start_height = start_height,
        num_threads = effective_num_threads,
        preload_pool_size = config.preload_pool_size,
        "Initializing FastNEAR stream"
    );

    let lake_config = builder.build().context("Failed to build FastNear config")?;
    let (handle, stream) = near_lake_framework::streamer(lake_config);

    info!("FastNEAR streamer created successfully");

    Ok((handle, stream))
}

#[cfg(test)]
mod tests {
    use super::{normalize_fastnear_start_height, NearChain};

    #[test]
    fn parse_chain_accepts_supported_values() {
        assert_eq!(NearChain::parse("mainnet").unwrap(), NearChain::Mainnet);
        assert_eq!(NearChain::parse("testnet").unwrap(), NearChain::Testnet);
    }

    #[test]
    fn parse_chain_rejects_unsupported_values() {
        assert!(NearChain::parse("disabled").is_err());
        assert!(NearChain::parse("staging").is_err());
    }

    #[test]
    fn normalize_fastnear_start_height_clamps_to_first_block() {
        assert_eq!(
            normalize_fastnear_start_height(NearChain::Mainnet, 0),
            9_820_210
        );
        assert_eq!(
            normalize_fastnear_start_height(NearChain::Testnet, 1),
            42_376_888
        );
    }

    #[test]
    fn normalize_fastnear_start_height_keeps_valid_height() {
        assert_eq!(
            normalize_fastnear_start_height(NearChain::Mainnet, 170_000_000),
            170_000_000
        );
    }
}
