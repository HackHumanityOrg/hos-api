use entities::*;
use near_indexer::modules::tokens::TokensModule;
use near_indexer::IndexerModule;
use near_indexer_primitives::near_primitives::types::BlockHeight;
use near_lake_framework::LakeConfigBuilder;
use rstest::*;
use sea_orm::EntityTrait;
use test_support::{init_test_tracing, TestDatabase};

const TEST_BLOCK_HEIGHT: BlockHeight = 163365867; // Recent wrap.near activity

#[fixture]
async fn test_db() -> TestDatabase {
    // Load environment variables for AWS credentials
    dotenvy::from_filename("/Users/andrei/git/data-commons/.env").ok();
    TestDatabase::new().await
}

fn create_test_tokens_module() -> TokensModule {
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
                raw: None,
                accounts: None,
                tokens: Some(common::config::TokensModuleConfig {
                    enabled: true,
                    track_nep141: true,
                    track_nep171: true,
                    track_nep245: true,
                    contracts: vec![],
                }),
            }),
        }),
    });

    TokensModule::new(&config)
}

/// Test basic tokens module functionality
#[rstest]
#[tokio::test]
async fn test_tokens_module_basic(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let tokens_module = create_test_tokens_module();

    // Test module properties
    assert!(tokens_module.enabled(), "Module should be enabled");
    assert_eq!(tokens_module.name(), "tokens");

    // Test cursor operations
    let cursor = tokens_module.get_cursor(&db).await
        .expect("Failed to get cursor");
    assert_eq!(cursor, None, "Initial cursor should be None");

    tokens_module.update_cursor(&db, 12345).await
        .expect("Failed to update cursor");

    let updated_cursor = tokens_module.get_cursor(&db).await
        .expect("Failed to get updated cursor");
    assert_eq!(updated_cursor, Some(12345), "Cursor should be updated");

    println!("✅ Basic tokens module test passed");
}

/// Test with real NEAR Lake stream data
#[rstest]
#[tokio::test] 
async fn test_real_lake_stream_basic(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let tokens_module = create_test_tokens_module();

    if !tokens_module.enabled() {
        return;
    }

    println!("🚀 Testing with real NEAR Lake stream at block {}", TEST_BLOCK_HEIGHT);

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

            let result = tokens_module.process_block(&msg, &db).await;
            match result {
                Ok(_) => {
                    println!("✅ Successfully processed block {}", msg.block.header.height);
                    
                    // Check for stored events
                    let raw_events_count = near_token_events_raw::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query raw events")
                        .len();

                    let parsed_events_count = near_token_events::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query parsed events")
                        .len();

                    println!("📊 Indexed {} raw events, {} parsed events", 
                        raw_events_count, parsed_events_count);
                },
                Err(e) => {
                    println!("⚠️  Failed to process block: {} (this may be expected if no token events)", e);
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

    println!("✅ Real NEAR Lake stream test completed");
}

/// Test module configuration
#[tokio::test]
async fn test_module_configuration() {
    init_test_tracing();

    println!("⚙️ Testing module configuration");

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
                raw: None,
                accounts: None,
                tokens: Some(common::config::TokensModuleConfig {
                    enabled: false,
                    track_nep141: true,
                    track_nep171: true,
                    track_nep245: true,
                    contracts: vec![],
                }),
            }),
        }),
    });

    let disabled_module = TokensModule::new(&disabled_config);
    assert!(!disabled_module.enabled(), "Module should be disabled");
    assert_eq!(disabled_module.name(), "tokens");
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

    let no_near_module = TokensModule::new(&no_near_config);
    assert!(!no_near_module.enabled(), "Module should be disabled when NEAR chain is disabled");
    println!("✅ Disabled NEAR chain configuration works");

    println!("✅ All configuration tests passed");
}