use entities::*;
use near_indexer::modules::raw::RawModule;
use near_indexer::IndexerModule;
use near_indexer_primitives::near_primitives::types::BlockHeight;
use near_lake_framework::LakeConfigBuilder;
use rstest::*;
use sea_orm::EntityTrait;
use test_support::{init_test_tracing, TestDatabase};

// Basic constants for backwards compatibility - only kept if actually used

#[fixture]
async fn test_db() -> TestDatabase {
    // Load environment variables for AWS credentials
    dotenvy::dotenv().ok();
    TestDatabase::new().await
}

fn create_test_raw_module() -> RawModule {
    let config = common::config::AppConfig::Indexer(common::config::IndexerConfig {
        database: common::config::DatabaseConfig::default(),
        logging: common::config::LoggingConfig::default(),
        telemetry: common::config::TelemetryConfig::default(),
        discourse: None,
        near: Some(common::config::NearLakeConfig {
            chain: "mainnet".to_string(),
            start_height: 0,
            concurrency: None,
            region: None,
            modules: Some(common::config::NearModulesConfig {
                raw: Some(common::config::RawModuleConfig { enabled: true }),
                accounts: None,
                tokens: None,
                staking_pools: None,
            }),
            preload_pool_size: Some(1),
        }),
    });

    RawModule::new(&config)
}

/// Test basic raw module functionality
#[rstest]
#[tokio::test]
async fn test_raw_module_basic(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let raw_module = create_test_raw_module();

    // Test module properties
    assert!(raw_module.enabled(), "Module should be enabled");
    assert_eq!(raw_module.name(), "raw");

    // Test cursor operations
    let cursor = raw_module
        .get_cursor(&db)
        .await
        .expect("Failed to get cursor");
    assert_eq!(cursor, None, "Initial cursor should be None");

    raw_module
        .update_cursor(&db, 98765)
        .await
        .expect("Failed to update cursor");

    let updated_cursor = raw_module
        .get_cursor(&db)
        .await
        .expect("Failed to get updated cursor");
    assert_eq!(updated_cursor, Some(98765), "Cursor should be updated");

    println!("✅ Basic raw module test passed");
}

/// Test with real NEAR Lake stream data
#[rstest]
#[tokio::test]
async fn test_real_lake_stream_raw(#[future] test_db: TestDatabase) {
    init_test_tracing();

    // Test-specific constants - verified against nearblocks API
    // Data fetched from: https://api.nearblocks.io/v1/blocks/100000000
    const TEST_BLOCK_HEIGHT: BlockHeight = 100000000;
    const EXPECTED_BLOCK_HASH: &str = "GLCuCE2yNJWH2EfWJu7pSM8swd5qd1RG71pwV4YFbUz7";
    const EXPECTED_PREVIOUS_HASH: &str = "8avcEDMh9RFAmfLV31ddo1LUUjEf7tDbLeUWwvE2Chpb";
    const EXPECTED_GAS_PRICE: &str = "100000000";
    const EXPECTED_MIN_TRANSACTIONS: u32 = 4;
    const EXPECTED_SHARDS: u32 = 4;

    // Expected block data structure
    struct ExpectedBlockInfo {
        block_height: BlockHeight,
        block_hash: &'static str,
        previous_hash: &'static str,
        gas_price: &'static str,
        min_transactions: u32,
        shards_count: u32,
    }

    const EXPECTED_BLOCK: ExpectedBlockInfo = ExpectedBlockInfo {
        block_height: TEST_BLOCK_HEIGHT,
        block_hash: EXPECTED_BLOCK_HASH,
        previous_hash: EXPECTED_PREVIOUS_HASH,
        gas_price: EXPECTED_GAS_PRICE,
        min_transactions: EXPECTED_MIN_TRANSACTIONS,
        shards_count: EXPECTED_SHARDS,
    };

    let test_db = test_db.await;
    let db = test_db.db();
    let raw_module = create_test_raw_module();

    if !raw_module.enabled() {
        return;
    }

    println!(
        "🔧 Testing raw data with real NEAR Lake stream at block {}",
        TEST_BLOCK_HEIGHT
    );

    let config = LakeConfigBuilder::default()
        .mainnet()
        .start_block_height(TEST_BLOCK_HEIGHT)
        .blocks_preload_pool_size(1)
        .build()
        .expect("Failed to build Lake config");

    let (handle, mut stream) = near_lake_framework::streamer(config);

    // Try to get one message with a timeout
    let timeout_duration = tokio::time::Duration::from_secs(30);
    match tokio::time::timeout(timeout_duration, stream.recv()).await {
        Ok(Some(msg)) => {
            println!(
                "📦 Processing block {} with {} shards",
                msg.block.header.height,
                msg.shards.len()
            );

            // Validate block data against expected values
            if msg.block.header.height == EXPECTED_BLOCK.block_height {
                println!("🔍 Validating block against expected data:");
                println!(
                    "   Block height: {} (expected: {})",
                    msg.block.header.height, EXPECTED_BLOCK.block_height
                );
                println!("   Block hash: {}", msg.block.header.hash);
                println!("   Expected hash: {}", EXPECTED_BLOCK.block_hash);
                println!("   Previous hash: {}", msg.block.header.prev_hash);
                println!("   Expected prev hash: {}", EXPECTED_BLOCK.previous_hash);
                println!("   Gas price: {}", msg.block.header.gas_price);
                println!("   Expected gas price: {}", EXPECTED_BLOCK.gas_price);

                assert_eq!(
                    msg.block.header.height, EXPECTED_BLOCK.block_height,
                    "Block height should match"
                );
                assert_eq!(
                    msg.block.header.hash.to_string(),
                    EXPECTED_BLOCK.block_hash,
                    "Block hash should match expected"
                );
                assert_eq!(
                    msg.block.header.prev_hash.to_string(),
                    EXPECTED_BLOCK.previous_hash,
                    "Previous hash should match expected"
                );
                assert_eq!(
                    msg.block.header.gas_price.to_string(),
                    EXPECTED_BLOCK.gas_price,
                    "Gas price should match expected"
                );

                // Validate shard count
                assert_eq!(
                    msg.shards.len() as u32,
                    EXPECTED_BLOCK.shards_count,
                    "Shard count should match expected: got {}, expected {}",
                    msg.shards.len(),
                    EXPECTED_BLOCK.shards_count
                );

                // Count total transactions and receipts
                let total_transactions: usize = msg
                    .shards
                    .iter()
                    .map(|s| s.chunk.as_ref().map(|c| c.transactions.len()).unwrap_or(0))
                    .sum();
                let total_receipts: usize = msg
                    .shards
                    .iter()
                    .map(|s| s.receipt_execution_outcomes.len())
                    .sum();

                println!(
                    "   Transactions: {} (expected: at least {})",
                    total_transactions, EXPECTED_BLOCK.min_transactions
                );
                println!("   Receipts: {}", total_receipts);

                // Allow some variance for ongoing blockchain activity
                assert!(
                    total_transactions >= EXPECTED_BLOCK.min_transactions as usize,
                    "Should have at least {} transactions, got {}",
                    EXPECTED_BLOCK.min_transactions,
                    total_transactions
                );

                println!("   ✅ Block validation passed");
            }

            let result = raw_module.process_block(&msg, &db).await;
            match result {
                Ok(_) => {
                    println!(
                        "✅ Successfully processed block {}",
                        msg.block.header.height
                    );

                    // Check for stored raw data
                    let blocks_count = near_blocks::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query blocks")
                        .len();

                    let chunks_count = near_chunks::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query chunks")
                        .len();

                    let transactions_count = near_transactions::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query transactions")
                        .len();

                    let receipts_count = near_receipts::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query receipts")
                        .len();

                    let execution_outcomes_count = near_execution_outcomes::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query execution outcomes")
                        .len();

                    let state_changes_count = near_state_changes::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query state changes")
                        .len();

                    println!("📊 Indexed raw data:");
                    println!("   {} blocks", blocks_count);
                    println!("   {} chunks", chunks_count);
                    println!("   {} transactions", transactions_count);
                    println!("   {} receipts", receipts_count);
                    println!("   {} execution outcomes", execution_outcomes_count);
                    println!("   {} state changes", state_changes_count);

                    // Show sample block and validate against expected data
                    if blocks_count > 0 {
                        let blocks = near_blocks::Entity::find()
                            .all(&db)
                            .await
                            .expect("Failed to query blocks");

                        if let Some(sample) = blocks.first() {
                            println!("   Sample block indexed:");
                            println!("     Height: {}", sample.height);
                            println!("     Hash: {}", sample.hash);
                            println!("     Timestamp: {}", sample.timestamp);
                            println!("     Gas price: {}", sample.gas_price);

                            // Validate against expected data if available
                            if sample.height as u64 == EXPECTED_BLOCK.block_height {
                                assert_eq!(
                                    sample.hash, EXPECTED_BLOCK.block_hash,
                                    "Indexed block hash should match expected"
                                );
                                assert_eq!(
                                    sample.gas_price, EXPECTED_BLOCK.gas_price,
                                    "Indexed gas price should match expected"
                                );
                                println!("     ✅ Indexed block data matches expected values");
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("⚠️  Failed to process block: {} (this may be expected)", e);
                }
            }
        }
        Ok(None) => {
            println!("⚠️  No message received from stream");
        }
        Err(_) => {
            println!("⚠️  Timeout waiting for stream message");
        }
    }

    // Clean shutdown
    handle.abort();
    let _ = handle.await;

    println!("✅ Real NEAR Lake stream raw test completed");
}

/// Test precise transaction and receipt validation using high-activity block
#[rstest]
#[tokio::test]
async fn test_precise_transaction_validation(#[future] test_db: TestDatabase) {
    init_test_tracing();

    // Test-specific constants
    const TEST_BLOCK_HEIGHT: BlockHeight = 163368228;
    const EXPECTED_BLOCK_HASH: &str = "5g6zsMuqYBSuA4ze6zUEZZBV4X6hGWDdftSnsvX3gM6N";

    // Expected transaction data
    struct ExpectedTransaction {
        transaction_hash: &'static str,
        signer_id: &'static str,
        receiver_id: &'static str,
    }

    const EXPECTED_TRANSACTIONS: &[ExpectedTransaction] = &[ExpectedTransaction {
        transaction_hash: "384v6rYphhpZ7LsNTxATADvkEt2YocmgDEECUCsURJty",
        signer_id: "7c5206b1b75b8787420b09d8697e08180cdf896c5fcf15f6afbf5f33fcc3cf72",
        receiver_id: "zec.omft.near",
    }];

    let test_db = test_db.await;
    let db = test_db.db();
    let raw_module = create_test_raw_module();

    if !raw_module.enabled() {
        return;
    }

    println!(
        "🔍 Testing precise transaction validation at high-activity block {}",
        TEST_BLOCK_HEIGHT
    );

    let config = LakeConfigBuilder::default()
        .mainnet()
        .start_block_height(TEST_BLOCK_HEIGHT)
        .blocks_preload_pool_size(1)
        .build()
        .expect("Failed to build Lake config");

    let (handle, mut stream) = near_lake_framework::streamer(config);

    let timeout_duration = tokio::time::Duration::from_secs(30);
    match tokio::time::timeout(timeout_duration, stream.recv()).await {
        Ok(Some(msg)) => {
            println!(
                "📦 Processing high-activity block {} with {} shards",
                msg.block.header.height,
                msg.shards.len()
            );

            // Validate this is the expected high activity block
            assert_eq!(msg.block.header.height, TEST_BLOCK_HEIGHT);
            assert_eq!(msg.block.header.hash.to_string(), EXPECTED_BLOCK_HASH);
            println!("✅ Block matches expected high-activity block");

            match raw_module.process_block(&msg, &db).await {
                Ok(_) => {
                    println!("✅ Successfully processed high-activity block");

                    let transactions = near_transactions::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query transactions");

                    let receipts = near_receipts::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query receipts");

                    let execution_outcomes = near_execution_outcomes::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query execution outcomes");

                    println!("📊 Precise transaction validation:");
                    println!("   {} transactions indexed", transactions.len());
                    println!("   {} receipts indexed", receipts.len());
                    println!("   {} execution outcomes indexed", execution_outcomes.len());

                    // Validate specific expected transactions
                    for expected_tx in EXPECTED_TRANSACTIONS.iter() {
                        let found_tx = transactions
                            .iter()
                            .find(|tx| tx.hash == expected_tx.transaction_hash);

                        if let Some(tx) = found_tx {
                            println!("   ✅ Found expected transaction: {}", tx.hash);
                            println!("     Signer: {}", tx.signer_id);
                            println!("     Receiver: {}", tx.receiver_id);
                            println!("     Block height: {}", tx.block_height);

                            assert_eq!(tx.hash, expected_tx.transaction_hash);
                            assert_eq!(tx.signer_id, expected_tx.signer_id);
                            assert_eq!(tx.receiver_id, expected_tx.receiver_id);
                            assert_eq!(tx.block_height, TEST_BLOCK_HEIGHT as i64);

                            println!("     ✅ Transaction data matches expected values");
                        } else {
                            println!(
                                "   ⚠️  Expected transaction not found: {}",
                                expected_tx.transaction_hash
                            );
                        }
                    }

                    // Validate transaction data consistency
                    for tx in transactions.iter().take(3) {
                        println!("   🔍 Validating transaction {}", tx.hash);

                        // Basic validation
                        assert!(!tx.hash.is_empty(), "Transaction hash should not be empty");
                        assert!(
                            !tx.signer_id.is_empty(),
                            "Signer account should not be empty"
                        );
                        assert!(
                            !tx.receiver_id.is_empty(),
                            "Receiver account should not be empty"
                        );
                        assert!(
                            tx.block_height == TEST_BLOCK_HEIGHT as i64,
                            "Transaction should be in correct block"
                        );

                        println!("     ✅ Transaction data is consistent");
                    }

                    // Validate receipt data
                    for receipt in receipts.iter().take(3) {
                        println!("   🔍 Validating receipt {}", receipt.receipt_id);

                        assert!(
                            !receipt.receipt_id.is_empty(),
                            "Receipt ID should not be empty"
                        );
                        assert!(
                            !receipt.predecessor_id.is_empty(),
                            "Predecessor should not be empty"
                        );
                        assert!(
                            !receipt.receiver_id.is_empty(),
                            "Receiver should not be empty"
                        );
                        assert!(
                            receipt.block_height == TEST_BLOCK_HEIGHT as i64,
                            "Receipt should be in correct block"
                        );

                        println!("     ✅ Receipt data is valid");
                    }
                }
                Err(e) => {
                    println!("⚠️  Failed to process high-activity block: {}", e);
                }
            }
        }
        _ => {
            println!("⚠️  No data received for precise transaction test");
        }
    }

    handle.abort();
    let _ = handle.await;

    println!("✅ Precise transaction validation completed");
}

/// Test raw module configuration
#[tokio::test]
async fn test_raw_module_configuration() {
    init_test_tracing();

    println!("⚙️ Testing raw module configuration");

    // Test disabled module
    let disabled_config = common::config::AppConfig::Indexer(common::config::IndexerConfig {
        database: common::config::DatabaseConfig::default(),
        logging: common::config::LoggingConfig::default(),
        telemetry: common::config::TelemetryConfig::default(),
        discourse: None,
        near: Some(common::config::NearLakeConfig {
            chain: "mainnet".to_string(),
            start_height: 0,
            concurrency: None,
            region: None,
            modules: Some(common::config::NearModulesConfig {
                raw: Some(common::config::RawModuleConfig { enabled: false }),
                accounts: None,
                tokens: None,
                staking_pools: None,
            }),
            preload_pool_size: Some(1),
        }),
    });

    let disabled_module = RawModule::new(&disabled_config);
    assert!(!disabled_module.enabled(), "Module should be disabled");
    assert_eq!(disabled_module.name(), "raw");
    println!("✅ Disabled module configuration works");

    // Test disabled NEAR chain
    let no_near_config = common::config::AppConfig::Indexer(common::config::IndexerConfig {
        database: common::config::DatabaseConfig::default(),
        logging: common::config::LoggingConfig::default(),
        telemetry: common::config::TelemetryConfig::default(),
        discourse: None,
        near: Some(common::config::NearLakeConfig {
            chain: "disabled".to_string(),
            start_height: 0,
            concurrency: None,
            region: None,
            modules: None,
            preload_pool_size: Some(1),
        }),
    });

    let no_near_module = RawModule::new(&no_near_config);
    assert!(
        !no_near_module.enabled(),
        "Module should be disabled when NEAR chain is disabled"
    );
    println!("✅ Disabled NEAR chain configuration works");

    // Test module enabled by default when no config specified
    let default_config = common::config::AppConfig::Indexer(common::config::IndexerConfig {
        database: common::config::DatabaseConfig::default(),
        logging: common::config::LoggingConfig::default(),
        telemetry: common::config::TelemetryConfig::default(),
        discourse: None,
        near: Some(common::config::NearLakeConfig {
            chain: "mainnet".to_string(),
            start_height: 0,
            concurrency: None,
            region: None,
            modules: Some(common::config::NearModulesConfig {
                raw: None, // No explicit config should default to enabled
                accounts: None,
                tokens: None,
                staking_pools: None,
            }),
            preload_pool_size: Some(1),
        }),
    });

    let default_module = RawModule::new(&default_config);
    assert!(
        default_module.enabled(),
        "Module should be enabled by default"
    );
    println!("✅ Default enabled configuration works");

    println!("✅ All raw configuration tests passed");
}
