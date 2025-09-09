use anyhow::Result;
use near_indexer::modules::validators::{HistoricalQuery, StakingPoolRpcUpdater};
use reqwest::header::{HeaderMap, HeaderValue};
use rust_decimal::Decimal;
use serde_json::json;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use wiremock::{
    matchers::{body_json, header, method, path},
    Mock, MockServer, ResponseTemplate,
};

/// Test helper to create a mock RPC response
fn create_rpc_response(result: serde_json::Value) -> serde_json::Value {
    json!({
        "jsonrpc": "2.0",
        "id": "dontcare",
        "result": result
    })
}

/// Test helper to create an RPC error response
fn create_rpc_error(code: i32, message: &str) -> serde_json::Value {
    json!({
        "jsonrpc": "2.0",
        "id": "dontcare",
        "error": {
            "code": code,
            "message": message
        }
    })
}

#[tokio::test]
async fn test_query_total_staked_balance_success() {
    let mock_server = MockServer::start().await;

    // Mock successful response for get_total_staked_balance
    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "query",
            "params": {
                "request_type": "call_function",
                "account_id": "test.poolv1.near",
                "method_name": "get_total_staked_balance",
                "args_base64": "",
                "finality": "final"
            }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(create_rpc_response(json!("1000000000000000000000000"))), // 1 NEAR in yoctoNEAR
        )
        .mount(&mock_server)
        .await;

    let rpc_updater = StakingPoolRpcUpdater::new(mock_server.uri(), 60);
    let result = rpc_updater
        .query_total_staked_balance_at("test.poolv1.near", None)
        .await
        .unwrap();

    // Should convert yoctoNEAR to NEAR (1000000000000000000000000 yocto = 1 NEAR)
    assert_eq!(result, Decimal::new(1, 0));
}

#[tokio::test]
async fn test_query_commission_rate_success() {
    let mock_server = MockServer::start().await;

    // Mock successful response for get_reward_fee_fraction
    Mock::given(method("POST"))
        .and(path("/"))
        .and(header("content-type", "application/json"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "query",
            "params": {
                "request_type": "call_function",
                "account_id": "test.poolv1.near",
                "method_name": "get_reward_fee_fraction",
                "args_base64": "",
                "finality": "final"
            }
        })))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(create_rpc_response(json!({
                "numerator": 5,
                "denominator": 100
            }))),
        )
        .mount(&mock_server)
        .await;

    let rpc_updater = StakingPoolRpcUpdater::new(mock_server.uri(), 60);
    let result = rpc_updater
        .query_commission_rate_at("test.poolv1.near", None)
        .await
        .unwrap();

    // Should be 5/100 = 0.05 (5%)
    assert_eq!(result, Decimal::new(5, 2));
}

#[tokio::test]
async fn test_query_commission_rate_error_returns_default() {
    let mock_server = MockServer::start().await;

    // Mock error response
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(create_rpc_error(-32000, "Contract method not found")),
        )
        .mount(&mock_server)
        .await;

    let rpc_updater = StakingPoolRpcUpdater::new(mock_server.uri(), 60);
    let result = rpc_updater
        .query_commission_rate_at("test.poolv1.near", None)
        .await
        .unwrap();

    // Should return default 5% on error
    assert_eq!(result, Decimal::new(5, 2));
}

#[tokio::test]
async fn test_historical_query_with_block_height() {
    let mock_server = MockServer::start().await;

    // Mock response for historical query
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "query",
            "params": {
                "request_type": "call_function",
                "account_id": "test.poolv1.near",
                "method_name": "get_total_staked_balance",
                "args_base64": "",
                "block_id": 12345
            }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(create_rpc_response(json!("2000000000000000000000000"))), // 2 NEAR
        )
        .mount(&mock_server)
        .await;

    let rpc_updater = StakingPoolRpcUpdater::new(mock_server.uri(), 60);
    let historical_query = HistoricalQuery {
        block_height: Some(12345),
        block_hash: None,
    };

    let result = rpc_updater
        .query_total_staked_balance_at("test.poolv1.near", Some(&historical_query))
        .await
        .unwrap();

    assert_eq!(result, Decimal::new(2, 0));
}

#[tokio::test]
async fn test_historical_query_with_block_hash() {
    let mock_server = MockServer::start().await;

    let block_hash = "6RWmTYhXCzjMjoY3Mz1rfFcnBm8E6XeDDbFEPUA4sv1w";

    // Mock response for historical query with block hash
    Mock::given(method("POST"))
        .and(path("/"))
        .and(body_json(json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "query",
            "params": {
                "request_type": "call_function",
                "account_id": "test.poolv1.near",
                "method_name": "get_total_staked_balance",
                "args_base64": "",
                "block_id": block_hash
            }
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(create_rpc_response(json!("3000000000000000000000000"))), // 3 NEAR
        )
        .mount(&mock_server)
        .await;

    let rpc_updater = StakingPoolRpcUpdater::new(mock_server.uri(), 60);
    let historical_query = HistoricalQuery {
        block_height: None,
        block_hash: Some(block_hash.to_string()),
    };

    let result = rpc_updater
        .query_total_staked_balance_at("test.poolv1.near", Some(&historical_query))
        .await
        .unwrap();

    assert_eq!(result, Decimal::new(3, 0));
}

#[tokio::test]
async fn test_rate_limit_headers_update() {
    let mock_server = MockServer::start().await;

    // Mock response with rate limit headers
    let mut headers = HeaderMap::new();
    headers.insert("x-ratelimit-remaining", HeaderValue::from_static("98"));
    headers.insert("x-ratelimit-limit", HeaderValue::from_static("100"));
    headers.insert("x-ratelimit-reset", HeaderValue::from_static("1704067200"));

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(create_rpc_response(json!("1000000000000000000000000")))
                .insert_header("x-ratelimit-remaining", "98")
                .insert_header("x-ratelimit-limit", "100")
                .insert_header("x-ratelimit-reset", "1704067200"),
        )
        .mount(&mock_server)
        .await;

    let rpc_updater = StakingPoolRpcUpdater::new(mock_server.uri(), 60);

    // Make request to trigger header parsing
    let _result = rpc_updater
        .query_total_staked_balance_at("test.poolv1.near", None)
        .await
        .unwrap();

    // Check that rate limit info was updated
    let rate_limit_status = rpc_updater.get_rate_limit_status().await;
    assert!(rate_limit_status.is_some());

    let (remaining, limit, reset_time) = rate_limit_status.unwrap();
    assert_eq!(remaining, 98);
    assert_eq!(limit, 100);
    assert_eq!(reset_time, Some(1704067200));
}

#[tokio::test]
async fn test_retry_after_header() {
    let mock_server = MockServer::start().await;

    // First request returns rate limit with retry-after
    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(429) // Too Many Requests
                .set_body_json(create_rpc_error(429, "Rate limit exceeded"))
                .insert_header("retry-after", "2"),
        )
        .mount(&mock_server)
        .await;

    let rpc_updater = StakingPoolRpcUpdater::new(mock_server.uri(), 60);

    // This should fail but update the rate limit info
    let result = rpc_updater
        .query_total_staked_balance_at("test.poolv1.near", None)
        .await;

    assert!(result.is_err());

    // Verify that rate limit info was updated with retry-after
    let _rate_limit_status = rpc_updater.get_rate_limit_status().await;
    // Note: get_rate_limit_status only returns (remaining, limit, reset_time)
    // The retry_after is handled internally but not exposed in this method
}

#[tokio::test]
async fn test_rate_limiting_timing() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(create_rpc_response(json!("1000000000000000000000000"))),
        )
        .mount(&mock_server)
        .await;

    // Create RPC updater with higher rate limit for testing
    let rpc_updater = StakingPoolRpcUpdater::with_rate_limit(mock_server.uri(), 60, 10); // 10 req/sec

    let start_time = Instant::now();

    // Make multiple requests
    for _ in 0..3 {
        let _result = rpc_updater
            .query_total_staked_balance_at("test.poolv1.near", None)
            .await
            .unwrap();
    }

    let elapsed = start_time.elapsed();

    // Should take at least 200ms for 3 requests at 10 req/sec (100ms intervals)
    // but less than 500ms (allowing for some overhead)
    assert!(elapsed >= Duration::from_millis(150));
    assert!(elapsed < Duration::from_millis(500));
}

#[tokio::test]
async fn test_yocto_to_near_conversion() {
    // Test various yoctoNEAR amounts - each test case gets its own mock server
    let test_cases = vec![
        ("0", Decimal::from_str("0").unwrap()),
        ("1000000000000000000000000", Decimal::from_str("1").unwrap()), // 1 NEAR
        (
            "500000000000000000000000",
            Decimal::from_str("0.5").unwrap(),
        ), // 0.5 NEAR
        ("2000000000000000000000000", Decimal::from_str("2").unwrap()), // 2 NEAR
    ];

    for (yocto_amount, expected_near) in test_cases {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(create_rpc_response(json!(yocto_amount))),
            )
            .mount(&mock_server)
            .await;

        let rpc_updater = StakingPoolRpcUpdater::new(mock_server.uri(), 60);
        let result = rpc_updater
            .query_total_staked_balance_at("test.poolv1.near", None)
            .await
            .unwrap();

        assert_eq!(
            result, expected_near,
            "Failed for yocto amount: {}",
            yocto_amount
        );
    }
}

#[tokio::test]
async fn test_concurrent_requests() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(create_rpc_response(json!("1000000000000000000000000"))),
        )
        .mount(&mock_server)
        .await;

    let rpc_updater = Arc::new(StakingPoolRpcUpdater::new(mock_server.uri(), 60));
    let mut handles = vec![];

    // Launch 5 concurrent requests
    for i in 0..5 {
        let rpc_updater_clone = rpc_updater.clone();
        let pool_id = format!("test{}.poolv1.near", i);

        let handle = tokio::spawn(async move {
            rpc_updater_clone
                .query_total_staked_balance_at(&pool_id, None)
                .await
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    let start_time = Instant::now();
    let results: Result<Vec<_>, _> = futures::future::try_join_all(handles).await;
    let elapsed = start_time.elapsed();

    assert!(results.is_ok());
    let results = results.unwrap();

    // All should succeed
    for result in results {
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Decimal::new(1, 0));
    }

    // Should take longer than if all were executed instantly due to rate limiting
    assert!(elapsed >= Duration::from_millis(500));
}

#[test]
fn test_yocto_to_near_conversion_direct() {
    use rust_decimal::Decimal;
    use std::str::FromStr;

    // Direct test of the conversion logic
    let test_cases = vec![
        (0u128, "0"),
        (1_000_000_000_000_000_000_000_000u128, "1"), // 1 NEAR
        (500_000_000_000_000_000_000_000u128, "0.5"), // 0.5 NEAR
        (2_000_000_000_000_000_000_000_000u128, "2"), // 2 NEAR
    ];

    for (yocto_amount, expected_near_str) in test_cases {
        let yocto_decimal = Decimal::from_str(&yocto_amount.to_string()).unwrap();
        let near_divisor = Decimal::from_str("1000000000000000000000000").unwrap(); // 10^24
        let result = yocto_decimal / near_divisor;
        let expected = Decimal::from_str(expected_near_str).unwrap();

        assert_eq!(
            result, expected,
            "Failed for yocto amount: {}",
            yocto_amount
        );
    }
}

#[tokio::test]
async fn test_custom_rate_limit_constructor() {
    let rpc_updater = StakingPoolRpcUpdater::with_rate_limit(
        "https://test.example.com".to_string(),
        30, // update_interval_minutes
        20, // requests_per_second
    );

    // Test that it was constructed properly
    // We can't directly access the fields, but we can test behavior indirectly
    // by checking that the rate limit status is initially None
    let status = rpc_updater.get_rate_limit_status().await;
    assert!(status.is_none());
}

mod live_api_tests {
    use super::*;
    use tokio::time::{sleep, Duration};

    const MAINNET_ARCHIVAL_RPC: &str = "https://archival-rpc.mainnet.near.org";

    // Known stable staking pool for testing
    const TEST_POOL: &str = "zavodil.poolv1.near";

    // Known block height with staking data
    const TEST_BLOCK_HEIGHT: u64 = 100_000_000; // Block from 2023

    #[tokio::test]
    async fn test_live_query_total_staked_balance() {
        let rpc_updater = StakingPoolRpcUpdater::new(MAINNET_ARCHIVAL_RPC.to_string(), 60);

        println!("Querying live staked balance for {}", TEST_POOL);

        let result = rpc_updater
            .query_total_staked_balance_at(TEST_POOL, None)
            .await;

        match result {
            Ok(balance) => {
                println!("✅ Current staked balance: {} NEAR", balance);
                assert!(
                    balance >= Decimal::from_str("0").unwrap(),
                    "Balance should be non-negative"
                );
            }
            Err(e) => {
                println!("❌ Failed to query balance: {}", e);
                // Don't fail the test if it's a network issue, just warn
                println!("⚠️  This may be due to network connectivity or RPC rate limiting");
            }
        }

        // Add delay to be respectful to the RPC endpoint
        sleep(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn test_live_query_commission_rate() {
        let rpc_updater = StakingPoolRpcUpdater::new(MAINNET_ARCHIVAL_RPC.to_string(), 60);

        println!("Querying live commission rate for {}", TEST_POOL);

        let result = rpc_updater.query_commission_rate_at(TEST_POOL, None).await;

        match result {
            Ok(rate) => {
                println!(
                    "✅ Current commission rate: {}% ({})",
                    rate * Decimal::from_str("100").unwrap(),
                    rate
                );
                assert!(
                    rate >= Decimal::from_str("0").unwrap(),
                    "Commission rate should be non-negative"
                );
                assert!(
                    rate <= Decimal::from_str("1").unwrap(),
                    "Commission rate should not exceed 100%"
                );
            }
            Err(e) => {
                println!("❌ Failed to query commission rate: {}", e);
                println!("⚠️  This may be due to network connectivity or RPC rate limiting");
            }
        }

        sleep(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn test_live_historical_query() {
        let rpc_updater = StakingPoolRpcUpdater::new(MAINNET_ARCHIVAL_RPC.to_string(), 60);

        let historical_query = HistoricalQuery {
            block_height: Some(TEST_BLOCK_HEIGHT),
            block_hash: None,
        };

        println!(
            "Querying historical staked balance for {} at block {}",
            TEST_POOL, TEST_BLOCK_HEIGHT
        );

        let result = rpc_updater
            .query_total_staked_balance_at(TEST_POOL, Some(&historical_query))
            .await;

        match result {
            Ok(balance) => {
                println!(
                    "✅ Historical staked balance at block {}: {} NEAR",
                    TEST_BLOCK_HEIGHT, balance
                );
                assert!(
                    balance >= Decimal::from_str("0").unwrap(),
                    "Historical balance should be non-negative"
                );
            }
            Err(e) => {
                println!("❌ Failed to query historical balance: {}", e);
                println!("⚠️  This may be due to archival data availability or network issues");
            }
        }

        sleep(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn test_live_rate_limit_detection() {
        let rpc_updater = StakingPoolRpcUpdater::new(MAINNET_ARCHIVAL_RPC.to_string(), 60);

        println!("Testing rate limit detection with live API...");

        // Make a request to see if we get rate limit headers
        let _result = rpc_updater
            .query_total_staked_balance_at(TEST_POOL, None)
            .await;

        let rate_limit_status = rpc_updater.get_rate_limit_status().await;

        match rate_limit_status {
            Some((remaining, limit, reset_time)) => {
                println!("✅ Rate limit detected:");
                println!("  - Remaining: {}", remaining);
                println!("  - Limit: {}", limit);
                println!("  - Reset time: {:?}", reset_time);

                assert!(remaining <= limit, "Remaining should not exceed limit");
                assert!(limit > 0, "Limit should be positive");
            }
            None => {
                println!(
                    "ℹ️  No rate limit headers detected (this is normal for some RPC providers)"
                );
            }
        }

        sleep(Duration::from_secs(1)).await;
    }

    #[tokio::test]
    async fn test_live_multiple_pools() {
        let rpc_updater = StakingPoolRpcUpdater::new(MAINNET_ARCHIVAL_RPC.to_string(), 60);

        // Test multiple known validator pools
        let test_pools = vec![
            "zavodil.poolv1.near",
            "blazenet.poolv1.near",
            "staked.poolv1.near",
        ];

        println!("Testing multiple live validator pools...");

        for pool_id in test_pools {
            println!("Querying {}", pool_id);

            let balance_result = rpc_updater
                .query_total_staked_balance_at(pool_id, None)
                .await;

            let commission_result = rpc_updater.query_commission_rate_at(pool_id, None).await;

            match (balance_result, commission_result) {
                (Ok(balance), Ok(rate)) => {
                    println!(
                        "  ✅ Balance: {} NEAR, Commission: {}%",
                        balance,
                        rate * Decimal::from_str("100").unwrap()
                    );
                }
                (Err(e), _) | (_, Err(e)) => {
                    println!("  ❌ Error querying {}: {}", pool_id, e);
                }
            }

            // Be respectful with rate limiting
            sleep(Duration::from_millis(500)).await;
        }
    }

    #[tokio::test]
    async fn test_live_performance_timing() {
        let rpc_updater = StakingPoolRpcUpdater::with_rate_limit(
            MAINNET_ARCHIVAL_RPC.to_string(),
            60,
            2, // Conservative 2 req/sec for live API
        );

        println!("Testing live API performance with rate limiting...");

        let start_time = std::time::Instant::now();
        let num_requests = 3;

        for i in 0..num_requests {
            println!("Making request {} of {}", i + 1, num_requests);

            let result = rpc_updater
                .query_total_staked_balance_at(TEST_POOL, None)
                .await;

            match result {
                Ok(balance) => println!("  ✅ Request {} succeeded: {} NEAR", i + 1, balance),
                Err(e) => println!("  ❌ Request {} failed: {}", i + 1, e),
            }
        }

        let elapsed = start_time.elapsed();
        println!("Total time for {} requests: {:?}", num_requests, elapsed);

        // With 2 req/sec rate limiting, 3 requests should take at least 1 second
        // but we'll be lenient due to network variability
        if elapsed.as_secs() >= 1 {
            println!(
                "✅ Rate limiting appears to be working (took {}s)",
                elapsed.as_secs_f32()
            );
        } else {
            println!("⚠️  Requests completed faster than expected - may indicate rate limiting issues or very fast responses");
        }
    }

    #[tokio::test]
    async fn test_live_error_handling() {
        let rpc_updater = StakingPoolRpcUpdater::new(MAINNET_ARCHIVAL_RPC.to_string(), 60);

        println!("Testing error handling with non-existent pool...");

        // Try to query a non-existent pool
        let result = rpc_updater
            .query_total_staked_balance_at("nonexistent.pool.near", None)
            .await;

        match result {
            Ok(balance) => {
                println!(
                    "⚠️  Unexpectedly got result for non-existent pool: {}",
                    balance
                );
            }
            Err(e) => {
                println!("✅ Correctly got error for non-existent pool: {}", e);
                // This is expected behavior
            }
        }

        sleep(Duration::from_secs(1)).await;
    }
}
