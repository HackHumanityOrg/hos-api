#![cfg(feature = "live_rpc_tests")]

use anyhow::{Context, Result};
use near_indexer::core::lake_stream::{create_fastnear_stream, LakeStreamConfig};
use serde_json::Value;
use std::time::Duration;
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

fn value_as_u64(value: &Value) -> Option<u64> {
    match value {
        Value::Number(number) => number.as_u64(),
        Value::String(string) => string.parse::<u64>().ok(),
        _ => None,
    }
}

async fn fetch_latest_final_height(chain: &str) -> Result<u64> {
    let endpoint = match chain {
        "testnet" => "https://testnet.neardata.xyz",
        _ => "https://mainnet.neardata.xyz",
    };
    let url = format!("{endpoint}/v0/last_block/final");

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
    let (streamer_handle, stream) = create_fastnear_stream(
        LakeStreamConfig {
            chain: chain.clone(),
            start_height,
            concurrency: 1,
            preload_pool_size: 4,
        },
        api_key,
    )
    .context("failed to create FastNEAR stream")?;

    let streamer_handle = streamer_handle;
    let mut stream = ReceiverStream::new(stream);
    let mut processed = 0u64;
    let mut previous_height: Option<u64> = None;
    let mut saw_non_empty_shards = false;

    while processed < max_blocks {
        let next_message = tokio::time::timeout(Duration::from_secs(45), stream.next())
            .await
            .context("timed out waiting for FastNEAR block")?;

        let Some(message) = next_message else {
            anyhow::bail!("FastNEAR stream ended before expected block count");
        };

        let height = message.block.header.height;
        if let Some(previous) = previous_height {
            assert!(
                height > previous,
                "expected strictly increasing heights, got {} then {}",
                previous,
                height
            );
        }

        if !message.shards.is_empty() {
            saw_non_empty_shards = true;
        }

        previous_height = Some(height);
        processed += 1;
    }

    if !streamer_handle.is_finished() {
        streamer_handle.abort();
    }

    assert_eq!(
        processed, max_blocks,
        "expected to process requested blocks"
    );
    assert!(
        saw_non_empty_shards,
        "expected at least one processed block with shard data"
    );

    Ok(())
}
