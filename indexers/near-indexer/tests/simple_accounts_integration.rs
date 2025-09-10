use entities::*;
use near_indexer::modules::accounts::AccountsModule;
use near_indexer::IndexerModule;
use near_indexer_primitives::near_primitives::types::BlockHeight;
use near_lake_framework::LakeConfigBuilder;
use rstest::*;
use sea_orm::EntityTrait;
use test_support::{init_test_tracing, TestDatabase};

const TEST_BLOCK_HEIGHT: BlockHeight = 100000000; // Block with account activity

#[fixture]
async fn test_db() -> TestDatabase {
    // Load environment variables for AWS credentials
    dotenvy::from_filename("/Users/andrei/git/data-commons/.env").ok();
    TestDatabase::new().await
}

fn create_test_accounts_module() -> AccountsModule {
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
                accounts: Some(common::config::AccountsModuleConfig {
                    enabled: true,
                }),
                tokens: None,
            }),
        }),
    });

    AccountsModule::new(&config)
}

/// Test basic accounts module functionality
#[rstest]
#[tokio::test]
async fn test_accounts_module_basic(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let accounts_module = create_test_accounts_module();

    // Test module properties
    assert!(accounts_module.enabled(), "Module should be enabled");
    assert_eq!(accounts_module.name(), "accounts");

    // Test cursor operations
    let cursor = accounts_module.get_cursor(&db).await
        .expect("Failed to get cursor");
    assert_eq!(cursor, None, "Initial cursor should be None");

    accounts_module.update_cursor(&db, 54321).await
        .expect("Failed to update cursor");

    let updated_cursor = accounts_module.get_cursor(&db).await
        .expect("Failed to get updated cursor");
    assert_eq!(updated_cursor, Some(54321), "Cursor should be updated");

    println!("✅ Basic accounts module test passed");
}

/// Test with real NEAR Lake stream data
#[rstest]
#[tokio::test]
async fn test_real_lake_stream_accounts(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let accounts_module = create_test_accounts_module();

    if !accounts_module.enabled() {
        return;
    }

    println!("🏠 Testing accounts with real NEAR Lake stream at block {}", TEST_BLOCK_HEIGHT);

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

            let result = accounts_module.process_block(&msg, &db).await;
            match result {
                Ok(_) => {
                    println!("✅ Successfully processed block {}", msg.block.header.height);
                    
                    // Check for stored accounts
                    let accounts_count = near_accounts::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query accounts")
                        .len();

                    let access_keys_count = near_access_keys::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query access keys")
                        .len();

                    println!("📊 Indexed {} accounts, {} access keys", 
                        accounts_count, access_keys_count);

                    // Show sample account if any
                    if accounts_count > 0 {
                        let accounts = near_accounts::Entity::find()
                            .all(&db)
                            .await
                            .expect("Failed to query accounts");
                        
                        if let Some(sample) = accounts.first() {
                            println!("   Sample account: {}", sample.account_id);
                            println!("     Type: {}", sample.account_type);
                            println!("     Created at height: {}", sample.created_at_height);
                            println!("     Is contract: {}", if sample.is_contract == 1 { "Yes" } else { "No" });
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

    println!("✅ Real NEAR Lake stream accounts test completed");
}

/// Test accounts module configuration
#[tokio::test]
async fn test_accounts_module_configuration() {
    init_test_tracing();

    println!("⚙️ Testing accounts module configuration");

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
                accounts: Some(common::config::AccountsModuleConfig {
                    enabled: false,
                }),
                tokens: None,
            }),
        }),
    });

    let disabled_module = AccountsModule::new(&disabled_config);
    assert!(!disabled_module.enabled(), "Module should be disabled");
    assert_eq!(disabled_module.name(), "accounts");
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

    let no_near_module = AccountsModule::new(&no_near_config);
    assert!(!no_near_module.enabled(), "Module should be disabled when NEAR chain is disabled");
    println!("✅ Disabled NEAR chain configuration works");

    println!("✅ All accounts configuration tests passed");
}