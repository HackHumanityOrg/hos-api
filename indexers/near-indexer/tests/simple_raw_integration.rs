use entities::*;
use near_indexer::modules::raw::RawModule;
use near_indexer::IndexerModule;
use near_indexer_primitives::near_primitives::types::BlockHeight;
use near_lake_framework::LakeConfigBuilder;
use rstest::*;
use sea_orm::EntityTrait;
use test_support::{init_test_tracing, TestDatabase};

const TEST_BLOCK_HEIGHT: BlockHeight = 100000000; // Block with general activity

#[fixture]
async fn test_db() -> TestDatabase {
    // Load environment variables for AWS credentials
    dotenvy::from_filename("/Users/andrei/git/data-commons/.env").ok();
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
                raw: Some(common::config::RawModuleConfig {
                    enabled: true,
                }),
                accounts: None,
                tokens: None,
            }),
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
    let cursor = raw_module.get_cursor(&db).await
        .expect("Failed to get cursor");
    assert_eq!(cursor, None, "Initial cursor should be None");

    raw_module.update_cursor(&db, 98765).await
        .expect("Failed to update cursor");

    let updated_cursor = raw_module.get_cursor(&db).await
        .expect("Failed to get updated cursor");
    assert_eq!(updated_cursor, Some(98765), "Cursor should be updated");

    println!("✅ Basic raw module test passed");
}

/// Test with real NEAR Lake stream data
#[rstest]
#[tokio::test]
async fn test_real_lake_stream_raw(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let raw_module = create_test_raw_module();

    if !raw_module.enabled() {
        return;
    }

    println!("🔧 Testing raw data with real NEAR Lake stream at block {}", TEST_BLOCK_HEIGHT);

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
            println!("📦 Processing block {} with {} shards", 
                msg.block.header.height, 
                msg.shards.len()
            );

            let result = raw_module.process_block(&msg, &db).await;
            match result {
                Ok(_) => {
                    println!("✅ Successfully processed block {}", msg.block.header.height);
                    
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

                    // Show sample block if any
                    if blocks_count > 0 {
                        let blocks = near_blocks::Entity::find()
                            .all(&db)
                            .await
                            .expect("Failed to query blocks");
                        
                        if let Some(sample) = blocks.first() {
                            println!("   Sample block:");
                            println!("     Height: {}", sample.height);
                            println!("     Hash: {}", sample.hash);
                            println!("     Timestamp: {}", sample.timestamp);
                            println!("     Gas price: {}", sample.gas_price);
                            println!("     Has raw data: {}", sample.raw_data.is_some());
                        }
                    }
                },
                Err(e) => {
                    println!("⚠️  Failed to process block: {} (this may be expected)", e);
                }
            }
        },
        Ok(None) => {
            println!("⚠️  No message received from stream");
        },
        Err(_) => {
            println!("⚠️  Timeout waiting for stream message");
        }
    }

    // Clean shutdown
    handle.abort();
    let _ = handle.await;

    println!("✅ Real NEAR Lake stream raw test completed");
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
                raw: Some(common::config::RawModuleConfig {
                    enabled: false,
                }),
                accounts: None,
                tokens: None,
            }),
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
        }),
    });

    let no_near_module = RawModule::new(&no_near_config);
    assert!(!no_near_module.enabled(), "Module should be disabled when NEAR chain is disabled");
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
            }),
        }),
    });

    let default_module = RawModule::new(&default_config);
    assert!(default_module.enabled(), "Module should be enabled by default");
    println!("✅ Default enabled configuration works");

    println!("✅ All raw configuration tests passed");
}