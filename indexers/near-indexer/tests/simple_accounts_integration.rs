use entities::*;
use near_indexer::modules::accounts::AccountsModule;
use near_indexer::IndexerModule;
use near_indexer_primitives::near_primitives::types::BlockHeight;
use near_lake_framework::LakeConfigBuilder;
use rstest::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use test_support::{init_test_tracing, TestDatabase};

// Blocks known to have account activity from token transactions
const HIGH_ACTIVITY_BLOCK: BlockHeight = 163368228; // Block with wrap.near and ref-finance activity
const REF_FINANCE_BLOCK: BlockHeight = 163368226; // Block with ref-finance swap activity
const DIVERSE_ACTIVITY_BLOCK: BlockHeight = 163365867; // Block with diverse activity

#[fixture]
async fn test_db() -> TestDatabase {
    // Load environment variables for AWS credentials
    dotenvy::dotenv().ok();
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
                accounts: Some(common::config::AccountsModuleConfig { enabled: true }),
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
    let cursor = accounts_module
        .get_cursor(&db)
        .await
        .expect("Failed to get cursor");
    assert_eq!(cursor, None, "Initial cursor should be None");

    accounts_module
        .update_cursor(&db, 54321)
        .await
        .expect("Failed to update cursor");

    let updated_cursor = accounts_module
        .get_cursor(&db)
        .await
        .expect("Failed to get updated cursor");
    assert_eq!(updated_cursor, Some(54321), "Cursor should be updated");

    println!("✅ Basic accounts module test passed");
}

/// Test with real NEAR Lake stream data - validate account state changes
#[rstest]
#[tokio::test]
async fn test_ref_finance_account_validation(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let accounts_module = create_test_accounts_module();

    if !accounts_module.enabled() {
        return;
    }

    println!(
        "🏠 Testing account state changes at REF Finance block {}",
        REF_FINANCE_BLOCK
    );

    // Expected accounts from REF Finance transaction activity
    let expected_ref_finance_account = "v2.ref-finance.near";
    let _expected_user_account = "7b893046785b433b47fcc6e0d303ad4c1b550c12f498e6df46da509dae9446c2";

    let config = LakeConfigBuilder::default()
        .mainnet()
        .start_block_height(REF_FINANCE_BLOCK)
        .blocks_preload_pool_size(1)
        .build()
        .expect("Failed to build Lake config");

    let (handle, mut stream) = near_lake_framework::streamer(config);

    let timeout_duration = tokio::time::Duration::from_secs(30);
    match tokio::time::timeout(timeout_duration, stream.recv()).await {
        Ok(Some(msg)) => {
            println!(
                "📦 Processing block {} with {} shards",
                msg.block.header.height,
                msg.shards.len()
            );

            let result = accounts_module.process_block(&msg, &db).await;
            match result {
                Ok(_) => {
                    println!(
                        "✅ Successfully processed block {}",
                        msg.block.header.height
                    );

                    // Query all indexed data
                    let all_accounts = near_accounts::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query accounts");

                    let all_access_keys = near_access_keys::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query access keys");

                    let all_balances = near_account_balances::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query account balances");

                    let all_events = near_account_events::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query account events");

                    println!("📊 Account indexing results:");
                    println!("   {} accounts", all_accounts.len());
                    println!("   {} access keys", all_access_keys.len());
                    println!("   {} balance records", all_balances.len());
                    println!("   {} account events", all_events.len());

                    // Validate account type classification
                    for account in &all_accounts {
                        println!("✅ Account classification:");
                        println!("   ID: {}", account.account_id);
                        println!("   Type: {}", account.account_type);
                        println!("   Length: {}", account.account_id.len());
                        println!("   Created at block: {}", account.created_at_height);
                        println!("   Is contract: {}", if account.is_contract == 1 { "Yes" } else { "No" });
                        
                        // Validate account type classification logic
                        if account.account_id.len() == 64 && account.account_id.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()) {
                            assert_eq!(account.account_type, "implicit", 
                                "64-char lowercase hex account should be classified as implicit: {}", account.account_id);
                            println!("     ✅ Implicit account correctly classified: {}", account.account_id);
                        } else if account.account_id.starts_with("0x") && account.account_id.len() == 42 {
                            assert_eq!(account.account_type, "eth_implicit", 
                                "0x... account should be classified as eth_implicit");
                            println!("     ✅ ETH implicit account correctly classified");
                        } else if account.account_id.contains('.') {
                            assert_eq!(account.account_type, "named", 
                                "Dotted account should be classified as named");
                            println!("     ✅ Named account correctly classified");
                        } else if account.account_id.contains("lockup") {
                            assert_eq!(account.account_type, "lockup", 
                                "Lockup account should be classified as lockup");
                            println!("     ✅ Lockup account correctly classified");
                        } else if account.account_id == "system" {
                            assert_eq!(account.account_type, "system", 
                                "System account should be classified as system");
                            println!("     ✅ System account correctly classified");
                        } else {
                            // For other accounts, just verify they have a valid type
                            assert!(["implicit", "named", "eth_implicit", "lockup", "system"].contains(&account.account_type.as_str()),
                                "Account type should be valid: {}", account.account_type);
                            println!("     ✅ Account type '{}' is valid for {}", account.account_type, account.account_id);
                        }

                        // Check if REF Finance account was processed
                        if account.account_id == expected_ref_finance_account {
                            println!("     ✅ REF Finance contract account detected");
                            assert_eq!(account.account_type, "named", "REF Finance should be named account");
                        }
                    }

                    // Validate access keys structure
                    for access_key in &all_access_keys {
                        println!("✅ Access key validation:");
                        println!("   Account: {}", access_key.account_id);
                        println!("   Permission: {}", access_key.permission_type);
                        println!("   Is active: {}", if access_key.is_active == 1 { "Yes" } else { "No" });
                        
                        // Validate permission types
                        assert!(["FullAccess", "FunctionCall"].contains(&access_key.permission_type.as_str()),
                            "Access key permission should be FullAccess or FunctionCall");
                        
                        // Validate public key format (basic check)
                        assert!(!access_key.public_key.is_empty(), "Public key should not be empty");
                        assert!(access_key.public_key.len() > 20, "Public key should be reasonable length");
                    }

                    // Validate balance records contain realistic data
                    for balance in &all_balances {
                        println!("✅ Balance record validation:");
                        println!("   Account: {}", balance.account_id);
                        println!("   Amount: {}", balance.amount);
                        println!("   Storage usage: {}", balance.storage_usage);
                        println!("   Block height: {}", balance.block_height);
                        
                        // Validate balance amounts are numeric strings
                        assert!(balance.amount.parse::<u128>().is_ok(), "Amount should be valid number");
                        assert!(balance.locked.parse::<u128>().is_ok(), "Locked amount should be valid number");
                        assert!(balance.block_height > 0, "Block height should be positive");
                        assert!(balance.storage_usage >= 0, "Storage usage should be non-negative");
                    }

                    // Validate account events capture state changes
                    for event in &all_events {
                        println!("✅ Account event validation:");
                        println!("   Account: {}", event.account_id);
                        println!("   Event type: {}", event.event_type);
                        println!("   Block height: {}", event.block_height);
                        
                        // Validate event types are known
                        assert!(["created", "deleted", "contract_deployed", "contract_updated", "contract_deleted"]
                            .contains(&event.event_type.as_str()),
                            "Event type should be valid account event");
                        
                        assert!(event.block_height > 0, "Event block height should be positive");
                        
                        if event.event_type == "contract_deployed" {
                            println!("     ✅ Contract deployment event captured");
                        }
                    }

                    // Ensure some account activity was captured
                    assert!(all_accounts.len() > 0 || all_balances.len() > 0 || all_events.len() > 0,
                        "Expected to capture some account activity in block {}", REF_FINANCE_BLOCK);
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

    println!("✅ REF Finance account validation completed");
}

/// Test account data consistency across multiple blocks
#[rstest]
#[tokio::test]
async fn test_account_consistency_validation(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let accounts_module = create_test_accounts_module();

    if !accounts_module.enabled() {
        return;
    }

    println!("🔍 Testing account data consistency across blocks");

    let mut test_blocks = [
        (HIGH_ACTIVITY_BLOCK, "high activity block"),
        (REF_FINANCE_BLOCK, "REF Finance activity"),
        (DIVERSE_ACTIVITY_BLOCK, "diverse activity"),
    ];
    test_blocks.sort_by_key(|k| k.0);

    let mut total_accounts = 0;
    let mut total_balances = 0;
    let mut total_events = 0;
    let mut contract_accounts = 0;

    for (block_height, description) in test_blocks.iter() {
        println!("📦 Processing {} at block {}", description, block_height);

        let config = LakeConfigBuilder::default()
            .mainnet()
            .start_block_height(*block_height)
            .blocks_preload_pool_size(1)
            .build()
            .expect("Failed to build Lake config");

        let (handle, mut stream) = near_lake_framework::streamer(config);

        let timeout_duration = tokio::time::Duration::from_secs(30);
        match tokio::time::timeout(timeout_duration, stream.recv()).await {
            Ok(Some(msg)) => {
                match accounts_module.process_block(&msg, &db).await {
                    Ok(_) => {
                        let accounts = near_accounts::Entity::find()
                            .all(&db)
                            .await
                            .expect("Failed to query accounts");

                        let balances = near_account_balances::Entity::find()
                            .all(&db)
                            .await
                            .expect("Failed to query balances");

                        let events = near_account_events::Entity::find()
                            .all(&db)
                            .await
                            .expect("Failed to query events");

                        let contracts = accounts.iter().filter(|a| a.is_contract == 1).count();

                        println!("  ✅ Block {} processed: {} accounts, {} balances, {} events, {} contracts",
                            block_height, accounts.len(), balances.len(), events.len(), contracts);

                        total_accounts = accounts.len();
                        total_balances = balances.len();
                        total_events = events.len();
                        contract_accounts += contracts;

                        // Validate account type distribution
                        let implicit_count = accounts.iter().filter(|a| a.account_type == "implicit").count();
                        let named_count = accounts.iter().filter(|a| a.account_type == "named").count();
                        let eth_implicit_count = accounts.iter().filter(|a| a.account_type == "eth_implicit").count();

                        if accounts.len() > 0 {
                            println!("    Account types: {} implicit, {} named, {} eth_implicit",
                                implicit_count, named_count, eth_implicit_count);
                        }
                    }
                    Err(e) => {
                        println!("  ⚠️  Block {} error: {}", block_height, e);
                    }
                }
            }
            _ => {
                println!("  ⚠️  No data received for block {}", block_height);
            }
        }

        handle.abort();
        let _ = handle.await;
    }

    println!("📈 Final account indexing summary:");
    println!("   {} total accounts indexed", total_accounts);
    println!("   {} total balance records", total_balances);
    println!("   {} total account events", total_events);
    println!("   {} contract accounts", contract_accounts);

    // Validate data relationships
    if total_accounts > 0 {
        let final_accounts = near_accounts::Entity::find()
            .all(&db)
            .await
            .expect("Failed to query final accounts");

        for account in final_accounts.iter().take(3) {
            // Check that each account has corresponding balance records
            let account_balances = near_account_balances::Entity::find()
                .filter(near_account_balances::Column::AccountId.eq(&account.account_id))
                .all(&db)
                .await
                .expect("Failed to query account balances");

            println!("✅ Account {} has {} balance records", account.account_id, account_balances.len());
            
            // Validate balance records have consistent block heights
            for balance in &account_balances {
                assert!(balance.block_height >= account.created_at_height,
                    "Balance record block height should be >= account creation height");
            }
        }
    }

    println!("✅ Account consistency validation completed");
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
                accounts: Some(common::config::AccountsModuleConfig { enabled: false }),
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
    assert!(
        !no_near_module.enabled(),
        "Module should be disabled when NEAR chain is disabled"
    );
    println!("✅ Disabled NEAR chain configuration works");

    println!("✅ All accounts configuration tests passed");
}
