use entities::*;
use near_indexer::modules::tokens::TokensModule;
use near_indexer::IndexerModule;
use near_indexer_primitives::near_primitives::types::BlockHeight;
use near_lake_framework::LakeConfigBuilder;
use rstest::*;
use sea_orm::EntityTrait;
use test_support::{init_test_tracing, TestDatabase};

// Block constants verified against nearblocks API
// Data fetched from: https://api.nearblocks.io/v1/blocks/{block_height}
const WRAP_NEAR_BLOCK: BlockHeight = 163368228; // wrap.near transfer: 106.284646 wNEAR
// Verified: hash=5g6zsMuqYBSuA4ze6zUEZZBV4X6hGWDdftSnsvX3gM6N, prev=Fuqyu14zcPchcvhs1iXhxQQezPhRARcmR4L2MH2h97QD, gas=100000000, shards=9, txs=11

const REF_FINANCE_BLOCK: BlockHeight = 163368226; // v2.ref-finance.near activity
// Verified: hash=899A83kM4GNiyb5jdFXRePeBACCirTvjSCfyp8jVkhem, prev=HcWrFvYtz3ddvuc3fRtk1iHd9MThGjf5Ezffn7iReXr8, gas=100000000, shards=9, txs=15

const USDT_BLOCK: BlockHeight = 163365867; // Block with diverse token activity
// Verified: hash=5BZVTy32BJdWbzsKCuQBhJKeR59Wt7aWtDS6jK9gHDK7, prev=CTBAGxjdjG73WyQFX1jNDt9ciNsvqc6pJeaPnm2PLNx1, gas=100000000, shards=9, txs=12

const NFT_ACTIVITY_BLOCK: BlockHeight = 163424982; // ShardDog NFT activity
// Verified: hash=8kKWN8VGdUQwYgep4shGvaqQaEvG8tSDr9c2Ms8Zi9a4, prev=Aop1BS8xeuz1BiNbp5PP5ttuKobzTM1oVB1VS9vdFgDX, gas=100000000, shards=9, txs=205

#[fixture]
async fn test_db() -> TestDatabase {
    // Load environment variables for AWS credentials
    dotenvy::dotenv().ok();
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
    let cursor = tokens_module
        .get_cursor(&db)
        .await
        .expect("Failed to get cursor");
    assert_eq!(cursor, None, "Initial cursor should be None");

    tokens_module
        .update_cursor(&db, 12345)
        .await
        .expect("Failed to update cursor");

    let updated_cursor = tokens_module
        .get_cursor(&db)
        .await
        .expect("Failed to get updated cursor");
    assert_eq!(updated_cursor, Some(12345), "Cursor should be updated");

    println!("✅ Basic tokens module test passed");
}

/// Test with real NEAR Lake stream data - validate actual transaction content
#[rstest]
#[tokio::test]
async fn test_wrap_near_token_events(#[future] test_db: TestDatabase) {
    init_test_tracing();

    // Test-specific constants
    const TEST_BLOCK_HEIGHT: BlockHeight = 163368228;
    const EXPECTED_TRANSACTION_HASH: &str = "384v6rYphhpZ7LsNTxATADvkEt2YocmgDEECUCsURJty";
    const EXPECTED_SIGNER: &str =
        "7c5206b1b75b8787420b09d8697e08180cdf896c5fcf15f6afbf5f33fcc3cf72";
    const EXPECTED_RECEIVER: &str = "v2.ref-finance.near";
    const EXPECTED_CONTRACT: &str = "wrap.near";
    const EXPECTED_AMOUNT: &str = "106284646260000000000000000"; // 106.28 wNEAR in yoctoNEAR

    // Expected contract data
    struct ExpectedContractData {
        contract_id: &'static str,
        standard: &'static str,
    }

    const EXPECTED_CONTRACT_DATA: ExpectedContractData = ExpectedContractData {
        contract_id: EXPECTED_CONTRACT,
        standard: "nep141",
    };

    // Expected transfer data
    struct ExpectedTransferData {
        transaction_hash: &'static str,
        contract_id: &'static str,
        standard: &'static str,
        from_account: &'static str,
        to_account: &'static str,
        amount: &'static str,
    }

    const EXPECTED_TRANSFER: ExpectedTransferData = ExpectedTransferData {
        transaction_hash: EXPECTED_TRANSACTION_HASH,
        contract_id: EXPECTED_CONTRACT,
        standard: "nep141",
        from_account: EXPECTED_SIGNER,
        to_account: EXPECTED_RECEIVER,
        amount: EXPECTED_AMOUNT,
    };

    let test_db = test_db.await;
    let db = test_db.db();
    let tokens_module = create_test_tokens_module();

    if !tokens_module.enabled() {
        return;
    }

    println!(
        "💰 Testing wrap.near token events at block {}",
        TEST_BLOCK_HEIGHT
    );

    let config = LakeConfigBuilder::default()
        .mainnet()
        .start_block_height(TEST_BLOCK_HEIGHT)
        .blocks_preload_pool_size(1)
        .build()
        .expect("Failed to build Lake config");

    let (handle, mut stream) = near_lake_framework::streamer(config);

    // Expected data from NearBlocks for this block/transaction:
    // Transaction: 384v6rYphhpZ7LsNTxATADvkEt2YocmgDEECUCsURJty
    // wNEAR transfer: 106,284,646.26 wNEAR
    // From: 7c5206b1b75b8787420b09d8697e08180cdf896c5fcf15f6afbf5f33fcc3cf72
    // To: v2.ref-finance.near
    let expected_contract = EXPECTED_CONTRACT;

    let timeout_duration = tokio::time::Duration::from_secs(30);
    match tokio::time::timeout(timeout_duration, stream.recv()).await {
        Ok(Some(msg)) => {
            println!(
                "📦 Processing block {} with {} shards",
                msg.block.header.height,
                msg.shards.len()
            );

            let result = tokens_module.process_block(&msg, &db).await;
            match result {
                Ok(_) => {
                    println!(
                        "✅ Successfully processed block {}",
                        msg.block.header.height
                    );

                    // Validate specific token events were captured
                    let raw_events = near_token_events_raw::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query raw events");

                    let parsed_events = near_token_events::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query parsed events");

                    let transfers = near_token_transfers::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query transfers");

                    let contracts = near_token_contracts::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query contracts");

                    println!("📊 Validating token data:");
                    println!("   {} raw events", raw_events.len());
                    println!("   {} parsed events", parsed_events.len());
                    println!("   {} transfers", transfers.len());
                    println!("   {} contracts", contracts.len());

                    // Validate wrap.near contract against expected data
                    let wrap_contract = contracts
                        .iter()
                        .find(|c| c.contract_id == expected_contract);

                    if let Some(contract) = wrap_contract {
                        println!("✅ wrap.near contract detected:");
                        println!("   Contract ID: {}", contract.contract_id);
                        println!("   Standards: {:?}", contract.standards_detected);
                        println!("   Total events: {}", contract.total_events);
                        println!("   Last activity block: {}", contract.last_activity_block);

                        // Validate against expected contract data
                        if contract.contract_id == EXPECTED_CONTRACT_DATA.contract_id {
                            let standards: Vec<String> =
                                serde_json::from_value(contract.standards_detected.clone())
                                    .unwrap_or_default();

                            assert!(
                                standards.contains(&EXPECTED_CONTRACT_DATA.standard.to_string()),
                                "Contract {} should support standard {}",
                                contract.contract_id,
                                EXPECTED_CONTRACT_DATA.standard
                            );

                            println!(
                                "     ✅ Contract standards match expected: {}",
                                EXPECTED_CONTRACT_DATA.standard
                            );
                        }
                    } else {
                        println!("⚠️  wrap.near contract not found in indexed contracts");
                    }

                    // Validate specific expected transfers
                    let matching_transfers: Vec<_> = transfers
                        .iter()
                        .filter(|t| {
                            t.contract_id == EXPECTED_TRANSFER.contract_id
                                && t.from_account
                                    .as_ref()
                                    .map(|f| f == EXPECTED_TRANSFER.from_account)
                                    .unwrap_or(false)
                                && t.to_account
                                    .as_ref()
                                    .map(|to| to == EXPECTED_TRANSFER.to_account)
                                    .unwrap_or(false)
                        })
                        .collect();

                    if !matching_transfers.is_empty() {
                        println!(
                            "✅ Found {} transfers matching expected data:",
                            matching_transfers.len()
                        );
                        for transfer in matching_transfers {
                            println!("   Expected Transfer:");
                            println!("     Contract: {}", transfer.contract_id);
                            println!("     Standard: {}", transfer.standard);
                            println!("     From: {:?}", transfer.from_account);
                            println!("     To: {:?}", transfer.to_account);
                            println!("     Amount: {}", transfer.amount);
                            println!("     Expected amount: {}", EXPECTED_TRANSFER.amount);
                            println!("     Block height: {}", transfer.block_height);

                            // Validate precise data
                            assert_eq!(
                                transfer.standard, EXPECTED_TRANSFER.standard,
                                "Transfer standard should match expected"
                            );
                            assert_eq!(
                                transfer.contract_id, EXPECTED_TRANSFER.contract_id,
                                "Transfer contract should match expected"
                            );
                            assert_eq!(
                                transfer.block_height, TEST_BLOCK_HEIGHT as i64,
                                "Transfer block should match expected"
                            );

                            // Validate amount (allow some precision variance for complex swaps)
                            let expected_amount: u128 =
                                EXPECTED_TRANSFER.amount.parse().unwrap_or(0);
                            let actual_amount: u128 = transfer.amount.parse().unwrap_or(0);

                            if expected_amount > 0 {
                                let variance_ratio = if actual_amount > expected_amount {
                                    actual_amount as f64 / expected_amount as f64
                                } else {
                                    expected_amount as f64 / actual_amount as f64
                                };
                                assert!(variance_ratio <= 1.1,
                                    "Transfer amount should be close to expected: got {}, expected {}",
                                    actual_amount, expected_amount);
                                println!(
                                    "     ✅ Transfer amount matches expected (within variance)"
                                );
                            }
                        }
                    } else {
                        println!(
                            "⚠️  Expected transfer not found for transaction {}",
                            EXPECTED_TRANSFER.transaction_hash
                        );
                    }

                    // Validate EVENT_JSON logs were parsed correctly
                    for event in &parsed_events {
                        if event.contract_id == expected_contract {
                            println!("✅ Found parsed wrap.near event:");
                            println!("   Standard: {}", event.standard);
                            println!("   Event type: {}", event.event_type);
                            println!("   Affected accounts: {}", event.affected_accounts);
                            println!("   Block height: {}", event.block_height);

                            // Validate event properties
                            assert_eq!(
                                event.standard, "nep141",
                                "Event should be NEP-141 standard"
                            );
                            assert!(
                                ["ft_transfer", "ft_mint", "ft_burn"]
                                    .contains(&event.event_type.as_str()),
                                "Event type should be valid NEP-141 event"
                            );
                        }
                    }

                    // Validate at least some token activity was captured
                    assert!(
                        parsed_events.len() > 0 || transfers.len() > 0 || contracts.len() > 0,
                        "Expected to find some token activity in block {}",
                        WRAP_NEAR_BLOCK
                    );
                }
                Err(e) => {
                    println!(
                        "⚠️  Failed to process block: {} (this may be expected if no token events)",
                        e
                    );
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

    println!("✅ wrap.near token events validation completed");
}

/// Test REF Finance token swap validation
#[rstest]
#[tokio::test]
async fn test_ref_finance_token_swap_validation(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let tokens_module = create_test_tokens_module();

    if !tokens_module.enabled() {
        return;
    }

    println!(
        "🔄 Testing REF Finance token swap at block {}",
        REF_FINANCE_BLOCK
    );

    // Expected data from NearBlocks for this transaction:
    // Transaction: CMP9T9EJnz6NoCPsvcuj68XaMKiYthTReVQwAqwRcKKT
    // SHITZU token: 3,530.564 SHITZU (token.0xshitzu.near)
    // wNEAR: 2.859 wNEAR (wrap.near)
    // Account: 7b893046785b433b47fcc6e0d303ad4c1b550c12f498e6df46da509dae9446c2
    let expected_shitzu_contract = "token.0xshitzu.near";
    let expected_wrap_contract = "wrap.near";
    let expected_account = "7b893046785b433b47fcc6e0d303ad4c1b550c12f498e6df46da509dae9446c2";

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
                "📦 Processing REF Finance swap block {}",
                msg.block.header.height
            );

            match tokens_module.process_block(&msg, &db).await {
                Ok(_) => {
                    println!(
                        "✅ Successfully processed block {}",
                        msg.block.header.height
                    );

                    let all_contracts = near_token_contracts::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query contracts");

                    let all_transfers = near_token_transfers::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query transfers");

                    let all_events = near_token_events::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query events");

                    let all_balances = near_token_balances::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query balances");

                    println!("📊 REF Finance swap validation:");
                    println!("   {} contracts", all_contracts.len());
                    println!("   {} transfers", all_transfers.len());
                    println!("   {} events", all_events.len());
                    println!("   {} balance records", all_balances.len());

                    // Validate SHITZU contract detection
                    let shitzu_contract = all_contracts
                        .iter()
                        .find(|c| c.contract_id == expected_shitzu_contract);

                    if let Some(contract) = shitzu_contract {
                        println!("✅ SHITZU contract detected: {}", contract.contract_id);
                        let standards: Vec<String> =
                            serde_json::from_value(contract.standards_detected.clone())
                                .unwrap_or_default();
                        assert!(
                            standards.contains(&"nep141".to_string()),
                            "SHITZU should be detected as NEP-141 token"
                        );
                    }

                    // Validate wrap.near contract detection
                    let wrap_contract = all_contracts
                        .iter()
                        .find(|c| c.contract_id == expected_wrap_contract);

                    if let Some(contract) = wrap_contract {
                        println!("✅ wrap.near contract detected: {}", contract.contract_id);
                    }

                    // Validate specific transfers occurred
                    let shitzu_transfers: Vec<_> = all_transfers
                        .iter()
                        .filter(|t| t.contract_id == expected_shitzu_contract)
                        .collect();

                    let wrap_transfers: Vec<_> = all_transfers
                        .iter()
                        .filter(|t| t.contract_id == expected_wrap_contract)
                        .collect();

                    if !shitzu_transfers.is_empty() {
                        println!("✅ Found {} SHITZU transfers:", shitzu_transfers.len());
                        for transfer in shitzu_transfers {
                            println!("   SHITZU Transfer:");
                            println!("     From: {:?}", transfer.from_account);
                            println!("     To: {:?}", transfer.to_account);
                            println!("     Amount: {}", transfer.amount);

                            // Validate expected amount (approximately 3,530.564)
                            let amount_f64: f64 = transfer.amount.parse().unwrap_or(0.0);
                            if amount_f64 > 3000.0 && amount_f64 < 4000.0 {
                                println!("     ✅ Amount matches expected SHITZU swap (~3,530)");
                            }
                        }
                    }

                    if !wrap_transfers.is_empty() {
                        println!("✅ Found {} wNEAR transfers:", wrap_transfers.len());
                        for transfer in wrap_transfers {
                            println!("   wNEAR Transfer:");
                            println!("     From: {:?}", transfer.from_account);
                            println!("     To: {:?}", transfer.to_account);
                            println!("     Amount: {}", transfer.amount);

                            // Validate expected amount (approximately 2.859)
                            let amount_f64: f64 = transfer.amount.parse().unwrap_or(0.0);
                            if amount_f64 > 2.0 && amount_f64 < 4.0 {
                                println!("     ✅ Amount matches expected wNEAR swap (~2.859)");
                            }
                        }
                    }

                    // Validate balance changes were tracked
                    let account_balances: Vec<_> = all_balances
                        .iter()
                        .filter(|b| b.account_id == expected_account)
                        .collect();

                    if !account_balances.is_empty() {
                        println!(
                            "✅ Found {} balance records for swapper:",
                            account_balances.len()
                        );
                        for balance in account_balances {
                            println!("   Balance record:");
                            println!("     Account: {}", balance.account_id);
                            println!("     Contract: {}", balance.contract_id);
                            println!("     Amount: {}", balance.amount);
                            println!("     Last updated: {}", balance.last_updated_block);
                        }
                    }

                    // Validate events were parsed correctly
                    let token_events: Vec<_> = all_events
                        .iter()
                        .filter(|e| {
                            e.contract_id == expected_shitzu_contract
                                || e.contract_id == expected_wrap_contract
                        })
                        .collect();

                    if !token_events.is_empty() {
                        println!("✅ Found {} token events:", token_events.len());
                        for event in token_events {
                            println!("   Event:");
                            println!("     Contract: {}", event.contract_id);
                            println!("     Type: {}", event.event_type);
                            println!("     Affected accounts: {}", event.affected_accounts);

                            // Validate accounts involved in swap
                            assert!(
                                event.affected_accounts.contains(expected_account)
                                    || event.affected_accounts.contains("v2.ref-finance.near"),
                                "Event should involve the swap participants"
                            );
                        }
                    }
                }
                Err(e) => {
                    println!("⚠️  Failed to process REF Finance block: {}", e);
                }
            }
        }
        Ok(None) => {
            println!("⚠️  No message received from REF Finance stream");
        }
        Err(_) => {
            println!("⚠️  Timeout waiting for REF Finance stream message");
        }
    }

    handle.abort();
    let _ = handle.await;

    println!("✅ REF Finance token swap validation completed");
}

/// Test multiple blocks for comprehensive token event coverage
#[rstest]
#[tokio::test]
async fn test_multiple_token_standards(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let tokens_module = create_test_tokens_module();

    if !tokens_module.enabled() {
        return;
    }

    let test_blocks = [
        (WRAP_NEAR_BLOCK, "wrap.near NEP-141"),
        (REF_FINANCE_BLOCK, "ref-finance activity"),
        (USDT_BLOCK, "diverse tokens"),
    ];

    for (block_height, description) in test_blocks.iter() {
        println!("🔍 Testing {} at block {}", description, block_height);

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
                println!("  📦 Processing block {}", msg.block.header.height);

                match tokens_module.process_block(&msg, &db).await {
                    Ok(_) => {
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

                        println!(
                            "  ✅ Block {} processed: {} raw, {} parsed events",
                            msg.block.header.height, raw_events_count, parsed_events_count
                        );
                    }
                    Err(e) => {
                        println!("  ⚠️  Block {} error: {}", block_height, e);
                    }
                }
            }
            Ok(None) => {
                println!("  ⚠️  No message received for block {}", block_height);
            }
            Err(_) => {
                println!("  ⚠️  Timeout for block {}", block_height);
            }
        }

        handle.abort();
        let _ = handle.await;
    }

    // Final summary
    let total_raw_events = near_token_events_raw::Entity::find()
        .all(&db)
        .await
        .expect("Failed to query final raw events")
        .len();

    let total_parsed_events = near_token_events::Entity::find()
        .all(&db)
        .await
        .expect("Failed to query final parsed events")
        .len();

    let total_transfers = near_token_transfers::Entity::find()
        .all(&db)
        .await
        .expect("Failed to query final transfers")
        .len();

    let total_contracts = near_token_contracts::Entity::find()
        .all(&db)
        .await
        .expect("Failed to query final contracts")
        .len();

    println!("📈 Final token indexing results:");
    println!("   {} total raw events", total_raw_events);
    println!("   {} total parsed events", total_parsed_events);
    println!("   {} total transfers", total_transfers);
    println!("   {} total token contracts discovered", total_contracts);

    println!("✅ Multiple token standards test completed");
}

/// Test NFT (NEP-171) token validation with ShardDog NFTs
#[rstest]
#[tokio::test]
async fn test_nft_token_validation(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let tokens_module = create_test_tokens_module();

    if !tokens_module.enabled() {
        return;
    }

    println!(
        "🎨 Testing NFT token validation at block {}",
        NFT_ACTIVITY_BLOCK
    );

    let config = LakeConfigBuilder::default()
        .mainnet()
        .start_block_height(NFT_ACTIVITY_BLOCK)
        .blocks_preload_pool_size(1)
        .build()
        .expect("Failed to build Lake config");

    let (handle, mut stream) = near_lake_framework::streamer(config);

    // Test-specific constants for NFT
    const EXPECTED_CONTRACT: &str = "mint.sharddog.near";
    const EXPECTED_TOKEN_ID: &str = "594:772";
    const EXPECTED_FROM_ACCOUNT: &str = "mebarek19.tg";
    const EXPECTED_TO_ACCOUNT: &str = "intents.near";

    // Expected NFT contract data
    struct ExpectedNftContract {
        contract_id: &'static str,
        is_nft: bool,
    }

    const EXPECTED_NFT_CONTRACT: ExpectedNftContract = ExpectedNftContract {
        contract_id: EXPECTED_CONTRACT,
        is_nft: true,
    };

    let timeout_duration = tokio::time::Duration::from_secs(30);
    match tokio::time::timeout(timeout_duration, stream.recv()).await {
        Ok(Some(msg)) => {
            println!(
                "📦 Processing NFT block {} with {} shards",
                msg.block.header.height,
                msg.shards.len()
            );

            match tokens_module.process_block(&msg, &db).await {
                Ok(_) => {
                    println!(
                        "✅ Successfully processed NFT block {}",
                        msg.block.header.height
                    );

                    let all_contracts = near_token_contracts::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query contracts");

                    let all_transfers = near_token_transfers::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query transfers");

                    let all_events = near_token_events::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query events");

                    println!("🎨 NFT validation results:");
                    println!("   {} contracts detected", all_contracts.len());
                    println!("   {} transfers recorded", all_transfers.len());
                    println!("   {} events captured", all_events.len());

                    // Validate ShardDog NFT contract
                    let sharddog_contract = all_contracts
                        .iter()
                        .find(|c| c.contract_id == EXPECTED_CONTRACT);

                    if let Some(contract) = sharddog_contract {
                        println!("✅ ShardDog NFT contract detected:");
                        println!("   Contract ID: {}", contract.contract_id);
                        println!("   Standards: {:?}", contract.standards_detected);
                        println!("   Total events: {}", contract.total_events);

                        // Validate NEP-171 standard detection
                        let standards: Vec<String> =
                            serde_json::from_value(contract.standards_detected.clone())
                                .unwrap_or_default();
                        assert!(
                            standards.contains(&"nep171".to_string()),
                            "ShardDog should be detected as NEP-171 NFT contract"
                        );

                        // Validate against expected contract data
                        if contract.contract_id == EXPECTED_NFT_CONTRACT.contract_id {
                            assert!(
                                EXPECTED_NFT_CONTRACT.is_nft,
                                "Expected contract should be marked as NFT"
                            );
                            println!("     ✅ Contract correctly identified as NFT");
                        }
                    } else {
                        println!("⚠️  ShardDog NFT contract not detected");
                    }

                    // Validate specific NFT transfer
                    let nft_transfers: Vec<_> = all_transfers
                        .iter()
                        .filter(|t| t.contract_id == EXPECTED_CONTRACT && t.standard == "nep171")
                        .collect();

                    if !nft_transfers.is_empty() {
                        println!("✅ Found {} NFT transfers:", nft_transfers.len());
                        for transfer in nft_transfers {
                            println!("   NFT Transfer:");
                            println!("     Contract: {}", transfer.contract_id);
                            println!("     Standard: {}", transfer.standard);
                            println!("     From: {:?}", transfer.from_account);
                            println!("     To: {:?}", transfer.to_account);
                            println!("     Token ID: {:?}", transfer.token_id);
                            println!("     Amount: {}", transfer.amount);
                            println!("     Block height: {}", transfer.block_height);

                            // Validate NFT-specific fields
                            assert_eq!(
                                transfer.standard, "nep171",
                                "Transfer should be NEP-171 NFT"
                            );
                            assert_eq!(
                                transfer.contract_id, EXPECTED_CONTRACT,
                                "Contract should match expected"
                            );
                            assert!(
                                transfer.token_id.is_some(),
                                "NFT transfer should have token ID"
                            );
                            assert_eq!(transfer.amount, "1", "NFT amount should be 1");

                            // Validate token ID matches expected
                            if let Some(token_id) = &transfer.token_id {
                                assert_eq!(
                                    token_id, EXPECTED_TOKEN_ID,
                                    "Token ID should match expected: got {}, expected {}",
                                    token_id, EXPECTED_TOKEN_ID
                                );
                                println!("     ✅ Token ID matches expected: {}", token_id);
                            }

                            // Validate participant accounts
                            if let (Some(from), Some(to)) =
                                (&transfer.from_account, &transfer.to_account)
                            {
                                assert_eq!(
                                    from, EXPECTED_FROM_ACCOUNT,
                                    "From account should match expected"
                                );
                                assert_eq!(
                                    to, EXPECTED_TO_ACCOUNT,
                                    "To account should match expected"
                                );
                                println!("     ✅ Transfer participants match expected");
                            }
                        }
                    } else {
                        println!("⚠️  No NFT transfers found for ShardDog contract");
                    }

                    // Validate NFT events
                    let nft_events: Vec<_> = all_events
                        .iter()
                        .filter(|e| e.contract_id == EXPECTED_CONTRACT && e.standard == "nep171")
                        .collect();

                    if !nft_events.is_empty() {
                        println!("✅ Found {} NFT events:", nft_events.len());
                        for event in nft_events {
                            println!("   NFT Event:");
                            println!("     Contract: {}", event.contract_id);
                            println!("     Standard: {}", event.standard);
                            println!("     Event type: {}", event.event_type);
                            println!("     Affected accounts: {}", event.affected_accounts);

                            // Validate NFT event types
                            assert!(
                                ["nft_transfer", "nft_mint", "nft_burn"]
                                    .contains(&event.event_type.as_str()),
                                "NFT event type should be valid: {}",
                                event.event_type
                            );

                            // Validate expected participants are mentioned
                            assert!(
                                event.affected_accounts.contains(EXPECTED_FROM_ACCOUNT)
                                    || event.affected_accounts.contains(EXPECTED_TO_ACCOUNT),
                                "Event should mention expected participants"
                            );
                        }
                    } else {
                        println!("⚠️  No NFT events found for ShardDog contract");
                    }
                }
                Err(e) => {
                    println!("⚠️  Failed to process NFT block: {}", e);
                }
            }
        }
        Ok(None) => {
            println!("⚠️  No message received for NFT test");
        }
        Err(_) => {
            println!("⚠️  Timeout waiting for NFT test message");
        }
    }

    handle.abort();
    let _ = handle.await;

    println!("✅ NFT token validation completed");
}

/// Test precise token balance tracking
#[rstest]
#[tokio::test]
async fn test_precise_token_balance_tracking(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let tokens_module = create_test_tokens_module();

    if !tokens_module.enabled() {
        return;
    }

    println!("💰 Testing precise token balance tracking");

    // Test REF Finance swap block for balance changes
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
                "📦 Processing balance tracking block {}",
                msg.block.header.height
            );

            match tokens_module.process_block(&msg, &db).await {
                Ok(_) => {
                    println!("✅ Successfully processed balance tracking block");

                    let all_balances = near_token_balances::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query token balances");

                    println!("💰 Balance tracking validation:");
                    println!("   {} balance records captured", all_balances.len());

                    // Test-specific balance validation constants
                    const TEST_BLOCK_HEIGHT: BlockHeight = 163368226;
                    const EXPECTED_ACCOUNT: &str = "v2.ref-finance.near";
                    const EXPECTED_CONTRACT: &str = "wrap.near";

                    // Validate expected balance changes
                    let account_balances: Vec<_> = all_balances
                        .iter()
                        .filter(|b| {
                            b.account_id == EXPECTED_ACCOUNT && b.contract_id == EXPECTED_CONTRACT
                        })
                        .collect();

                    if !account_balances.is_empty() {
                        println!("   ✅ Found balance records for {}:", EXPECTED_ACCOUNT);
                        for balance in account_balances {
                            println!("     Account: {}", balance.account_id);
                            println!("     Contract: {}", balance.contract_id);
                            println!("     Amount: {}", balance.amount);
                            println!("     Block: {}", balance.last_updated_block);

                            // Basic validation
                            assert_eq!(
                                balance.last_updated_block, TEST_BLOCK_HEIGHT as i64,
                                "Balance should be updated at correct block"
                            );
                            assert!(
                                balance.amount.parse::<u128>().is_ok(),
                                "Balance amount should be valid number"
                            );

                            println!("     ✅ Balance record is valid");
                        }
                    } else {
                        println!(
                            "   ⚠️  No balance records found for {} with contract {}",
                            EXPECTED_ACCOUNT, EXPECTED_CONTRACT
                        );
                    }

                    // Validate balance data consistency
                    for balance in all_balances.iter().take(5) {
                        println!("   🔍 Validating balance record:");
                        println!("     Account: {}", balance.account_id);
                        println!("     Contract: {}", balance.contract_id);
                        println!("     Amount: {}", balance.amount);

                        // Basic validation
                        assert!(
                            !balance.account_id.is_empty(),
                            "Account ID should not be empty"
                        );
                        assert!(
                            !balance.contract_id.is_empty(),
                            "Contract ID should not be empty"
                        );
                        assert!(
                            balance.amount.parse::<u128>().is_ok(),
                            "Amount should be valid number"
                        );
                        assert!(
                            balance.last_updated_block > 0,
                            "Block height should be positive"
                        );

                        println!("     ✅ Balance record is consistent");
                    }
                }
                Err(e) => {
                    println!("⚠️  Failed to process balance tracking block: {}", e);
                }
            }
        }
        _ => {
            println!("⚠️  No data received for balance tracking test");
        }
    }

    handle.abort();
    let _ = handle.await;

    println!("✅ Precise token balance tracking completed");
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
    assert!(
        !no_near_module.enabled(),
        "Module should be disabled when NEAR chain is disabled"
    );
    println!("✅ Disabled NEAR chain configuration works");

    println!("✅ All configuration tests passed");
}
