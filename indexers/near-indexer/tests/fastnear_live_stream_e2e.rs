#![cfg(feature = "live_rpc_tests")]

use anyhow::{Context, Result};
use near_indexer::core::lake_stream::{create_fastnear_stream, LakeStreamConfig};
use serde_json::Value;
use std::time::Duration;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

fn endpoint_for_chain(chain: &str) -> &'static str {
    match chain {
        "testnet" => "https://testnet.neardata.xyz",
        _ => "https://mainnet.neardata.xyz",
    }
}

fn value_as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(string) => string.parse::<u64>().ok(),
        _ => None,
    }
}

async fn fetch_latest_final_height(chain: &str) -> Result<u64> {
    let url = format!("{}/v0/last_block/final", endpoint_for_chain(chain));

    let payload = reqwest::get(&url)
        .await
        .with_context(|| format!("failed requesting latest block from {url}"))?
        .json::<Value>()
        .await
        .with_context(|| format!("failed decoding latest block payload from {url}"))?;

    payload
        .pointer("/block/header/height")
        .and_then(value_as_u64)
        .context("latest block payload does not include /block/header/height")
}

async fn fetch_first_block_height(chain: &str) -> Result<u64> {
    let url = format!("{}/v0/first_block", endpoint_for_chain(chain));

    let payload = reqwest::get(&url)
        .await
        .with_context(|| format!("failed requesting first block from {url}"))?
        .json::<Value>()
        .await
        .with_context(|| format!("failed decoding first block payload from {url}"))?;

    payload
        .pointer("/block/header/height")
        .and_then(value_as_u64)
        .context("first block payload does not include /block/header/height")
}

async fn collect_heights(
    chain: &str,
    start_height: u64,
    max_blocks: u64,
    api_key: Option<String>,
) -> Result<(Vec<u64>, bool)> {
    let (streamer_handle, stream) = create_fastnear_stream(
        LakeStreamConfig {
            chain: chain.to_string(),
            start_height,
            concurrency: 1,
            preload_pool_size: 4,
        },
        api_key,
        None,
    )
    .context("failed to create FastNEAR stream")?;

    let mut stream = ReceiverStream::new(stream);
    let mut heights = Vec::with_capacity(max_blocks as usize);
    let mut saw_non_empty_shards = false;

    while heights.len() < max_blocks as usize {
        let next_message = tokio::time::timeout(Duration::from_secs(45), stream.next())
            .await
            .context("timed out waiting for FastNEAR block")?;

        let Some(message) = next_message else {
            anyhow::bail!("FastNEAR stream ended before expected block count");
        };

        heights.push(message.block.header.height);
        if !message.shards.is_empty() {
            saw_non_empty_shards = true;
        }
    }

    if !streamer_handle.is_finished() {
        streamer_handle.abort();
    }

    Ok((heights, saw_non_empty_shards))
}

fn assert_strictly_increasing(heights: &[u64]) {
    for window in heights.windows(2) {
        assert!(
            window[1] > window[0],
            "expected strictly increasing heights, got {} then {}",
            window[0],
            window[1]
        );
    }
}

#[tokio::test]
#[ignore = "hits real FastNEAR endpoint"]
async fn fastnear_live_stream_processes_blocks() -> Result<()> {
    let chain = std::env::var("NEAR_LIVE_TEST_CHAIN").unwrap_or_else(|_| "mainnet".to_string());
    let max_blocks = std::env::var("NEAR_LIVE_TEST_BLOCKS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5)
        .max(1);

    let latest_final_height = fetch_latest_final_height(&chain).await?;
    let start_height = latest_final_height.saturating_sub(max_blocks + 2);
    let api_key = std::env::var("FASTNEAR_API_KEY").ok();

    let (heights, saw_non_empty_shards) =
        collect_heights(&chain, start_height, max_blocks, api_key).await?;

    assert_eq!(
        heights.len() as u64,
        max_blocks,
        "expected to process requested blocks"
    );
    assert_strictly_increasing(&heights);
    assert!(
        saw_non_empty_shards,
        "expected at least one processed block with shard data"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "hits real FastNEAR endpoint"]
async fn fastnear_historical_backfill_processes_blocks() -> Result<()> {
    let chain = std::env::var("NEAR_LIVE_TEST_CHAIN").unwrap_or_else(|_| "mainnet".to_string());
    let max_blocks = std::env::var("NEAR_HISTORICAL_TEST_BLOCKS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(5)
        .max(1);
    let lookback_blocks = std::env::var("NEAR_HISTORICAL_LOOKBACK")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(200_000)
        .max(10_000);

    let latest_final_height = fetch_latest_final_height(&chain).await?;
    let start_height = latest_final_height.saturating_sub(lookback_blocks);
    let api_key = std::env::var("FASTNEAR_API_KEY").ok();

    let (heights, saw_non_empty_shards) =
        collect_heights(&chain, start_height, max_blocks, api_key).await?;

    assert_eq!(
        heights.len() as u64,
        max_blocks,
        "expected to process requested blocks"
    );
    assert_strictly_increasing(&heights);
    assert!(
        heights[0] >= start_height,
        "expected first processed height to be at/after requested historical start"
    );
    assert!(
        heights[0] < latest_final_height.saturating_sub(1_000),
        "expected historical run to start well before latest finalized block"
    );
    assert!(
        saw_non_empty_shards,
        "expected at least one processed historical block with shard data"
    );

    Ok(())
}

#[tokio::test]
#[ignore = "hits real FastNEAR endpoint"]
async fn fastnear_historical_from_first_block_processes_blocks() -> Result<()> {
    let chain = std::env::var("NEAR_LIVE_TEST_CHAIN").unwrap_or_else(|_| "mainnet".to_string());
    let max_blocks = std::env::var("NEAR_HISTORICAL_TEST_BLOCKS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3)
        .max(1);

    let first_block_height = fetch_first_block_height(&chain).await?;
    let api_key = std::env::var("FASTNEAR_API_KEY").ok();
    let (heights, saw_non_empty_shards) =
        collect_heights(&chain, first_block_height, max_blocks, api_key).await?;

    assert_eq!(
        heights.len() as u64,
        max_blocks,
        "expected to process requested blocks"
    );
    assert_eq!(
        heights[0], first_block_height,
        "expected first streamed height to match first historical block"
    );
    assert_strictly_increasing(&heights);
    assert!(
        saw_non_empty_shards,
        "expected at least one processed historical block with shard data"
    );

    Ok(())
}
