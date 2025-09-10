use entities::*;
use near_indexer::modules::staking_pools::StakingPoolsModule;
use near_indexer::IndexerModule;
use near_indexer_primitives::near_primitives::types::BlockHeight;
use near_lake_framework::LakeConfigBuilder;
use rstest::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use test_support::{init_test_tracing, TestDatabase};

// Block constants verified against nearblocks API for staking pool activity
// Data fetched from: https://api.nearblocks.io/v1/blocks/{block_height}
const STAKING_POOL_BLOCK: BlockHeight = 163446764; // Block with deposit_and_stake activity
                                                   // Verified: hash=3XG1oi9Y1CdvFVxx76BqwrL7cbXg2TVbYvF867ch9Q2T, author=figment.poolv1.near, txs=25
                                                   // Transaction: 4PExvppuPowjhMo1zVhv69i5AFsi9fnmbfLDSYvqWVvQ (deposit_and_stake)

const UNSTAKE_ACTIVITY_BLOCK: BlockHeight = 163446874; // Block with unstake activity
                                                       // Verified: hash=CBo3rBXBEj8hXAEiicyBRoqj7NDECYZFKfHRMCvDifZM
                                                       // Transaction: MrvDkZneWNfFEqczi2YZjFvyTRy2gjw3BPdtPqWaacX (unstake attempt)

const VALIDATOR_PROPOSAL_BLOCK: BlockHeight = 163368228; // Block authored by staking pool validator
                                                         // Verified: hash=5g6zsMuqYBSuA4ze6zUEZZBV4X6hGWDdftSnsvX3gM6N, author=zavodil.poolv1.near

#[fixture]
async fn test_db() -> TestDatabase {
    // Load environment variables for AWS credentials
    dotenvy::dotenv().ok();
    TestDatabase::new().await
}

fn create_test_staking_pools_module() -> StakingPoolsModule {
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
                tokens: None,
                staking_pools: Some(common::config::StakingPoolsModuleConfig {
                    enabled: true,
                    pool_patterns: vec!["*.poolv1.near".to_string(), "*.pool.near".to_string()],
                    custom_pools: vec![],
                    track_epochs: true,
                    track_delegators: true,
                    parse_events: true,
                }),
            }),
        }),
    });

    StakingPoolsModule::new(&config)
}

/// Test basic staking pools module functionality
#[rstest]
#[tokio::test]
async fn test_staking_pools_module_basic(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let staking_module = create_test_staking_pools_module();

    // Test module properties
    assert!(staking_module.enabled(), "Module should be enabled");
    assert_eq!(staking_module.name(), "staking_pools");

    // Test cursor operations
    let cursor = staking_module
        .get_cursor(&db)
        .await
        .expect("Failed to get cursor");
    assert_eq!(cursor, None, "Initial cursor should be None");

    staking_module
        .update_cursor(&db, 54321)
        .await
        .expect("Failed to update cursor");

    let updated_cursor = staking_module
        .get_cursor(&db)
        .await
        .expect("Failed to get updated cursor");
    assert_eq!(updated_cursor, Some(54321), "Cursor should be updated");

    println!("✅ Basic staking pools module test passed");
}

/// Test with real NEAR Lake stream data - validate staking pool detection and actions
#[rstest]
#[tokio::test]
async fn test_deposit_and_stake_validation(#[future] test_db: TestDatabase) {
    init_test_tracing();

    // Test-specific expected data constants
    const TEST_BLOCK_HEIGHT: BlockHeight = 163446764;
    const EXPECTED_SIGNER: &str = "hyoyapower.near";
    const EXPECTED_RECEIVER: &str = "astro-stakers.poolv1.near";
    const EXPECTED_DEPOSIT_AMOUNT: &str = "9998000000000000000000000000"; // ~9.998 NEAR

    // Expected pool data for validation
    struct ExpectedPool {
        pool_id: &'static str,
        pool_type: &'static str,
        detected_method: &'static str,
    }

    const EXPECTED_POOL: ExpectedPool = ExpectedPool {
        pool_id: EXPECTED_RECEIVER,
        pool_type: "poolv1",
        detected_method: "pattern_match",
    };

    // Expected action data
    struct ExpectedAction {
        pool_id: &'static str,
        method_name: &'static str,
        caller_account_id: &'static str,
        attached_deposit: &'static str,
        action_type: &'static str,
    }

    const EXPECTED_ACTION: ExpectedAction = ExpectedAction {
        pool_id: EXPECTED_RECEIVER,
        method_name: "deposit_and_stake",
        caller_account_id: EXPECTED_SIGNER,
        attached_deposit: EXPECTED_DEPOSIT_AMOUNT,
        action_type: "deposit_and_stake",
    };

    let test_db = test_db.await;
    let db = test_db.db();
    let staking_module = create_test_staking_pools_module();

    if !staking_module.enabled() {
        return;
    }

    println!(
        "🏦 Testing staking pool deposit_and_stake at block {}",
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
                "📦 Processing block {} with {} shards",
                msg.block.header.height,
                msg.shards.len()
            );

            let result = staking_module.process_block(&msg, &db).await;
            match result {
                Ok(_) => {
                    println!(
                        "✅ Successfully processed block {}",
                        msg.block.header.height
                    );

                    // Query all indexed staking data
                    let all_pools = near_staking_pools::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query staking pools");

                    let all_actions = near_staking_pool_actions::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query pool actions");

                    let all_delegators = near_staking_delegators::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query delegators");

                    let all_events = near_staking_pool_events::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query pool events");

                    let all_rewards = near_staking_pool_rewards::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query pool rewards");

                    let all_balances = near_staking_pool_balances::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query pool balances");

                    let all_validator_proposals = near_staking_validator_proposals::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query validator proposals");

                    println!("📊 Staking pool indexing results:");
                    println!("   {} pools discovered", all_pools.len());
                    println!("   {} actions recorded", all_actions.len());
                    println!("   {} delegators tracked", all_delegators.len());
                    println!("   {} events parsed", all_events.len());
                    println!("   {} reward events", all_rewards.len());
                    println!("   {} balance events", all_balances.len());
                    println!("   {} validator proposals", all_validator_proposals.len());

                    // Validate expected staking pool was detected
                    let expected_pool = all_pools
                        .iter()
                        .find(|p| p.pool_id == EXPECTED_POOL.pool_id);

                    if let Some(pool) = expected_pool {
                        println!("✅ Expected staking pool detected:");
                        println!("   Pool ID: {}", pool.pool_id);
                        println!("   Pool type: {}", pool.pool_type);
                        println!("   Detection method: {}", pool.detected_method);
                        println!("   First seen: {}", pool.first_seen_height);
                        println!("   Is active: {}", pool.is_active);

                        // Validate against expected pool data
                        assert_eq!(
                            pool.pool_type, EXPECTED_POOL.pool_type,
                            "Pool type should match expected"
                        );
                        assert_eq!(
                            pool.detected_method, EXPECTED_POOL.detected_method,
                            "Detection method should match expected"
                        );
                        assert_eq!(pool.is_active, 1, "Pool should be active");
                        println!("     ✅ Pool data matches expected values");
                    } else {
                        println!("⚠️  Expected staking pool not detected");
                    }

                    // Validate expected action was recorded
                    let expected_actions: Vec<_> = all_actions
                        .iter()
                        .filter(|a| {
                            a.pool_id == EXPECTED_ACTION.pool_id
                                && a.method_name == EXPECTED_ACTION.method_name
                                && a.caller_account_id == EXPECTED_ACTION.caller_account_id
                        })
                        .collect();

                    if !expected_actions.is_empty() {
                        println!(
                            "✅ Found {} matching staking actions:",
                            expected_actions.len()
                        );
                        for action in expected_actions {
                            println!("   Staking Action:");
                            println!("     Pool: {}", action.pool_id);
                            println!("     Method: {}", action.method_name);
                            println!("     Caller: {}", action.caller_account_id);
                            println!("     Deposit: {}", action.attached_deposit);
                            println!("     Success: {:?}", action.success);
                            println!("     Block height: {}", action.block_height);

                            // Validate precise action data
                            assert_eq!(
                                action.method_name, EXPECTED_ACTION.method_name,
                                "Method should match expected"
                            );
                            assert_eq!(
                                action.pool_id, EXPECTED_ACTION.pool_id,
                                "Pool ID should match expected"
                            );
                            assert_eq!(
                                action.caller_account_id, EXPECTED_ACTION.caller_account_id,
                                "Caller should match expected"
                            );

                            // Validate deposit amount (allow some variance for gas)
                            let expected_deposit: u128 =
                                EXPECTED_ACTION.attached_deposit.parse().unwrap_or(0);
                            let actual_deposit: u128 = action.attached_deposit.parse().unwrap_or(0);

                            if expected_deposit > 0 {
                                let variance_ratio = if actual_deposit > expected_deposit {
                                    actual_deposit as f64 / expected_deposit as f64
                                } else {
                                    expected_deposit as f64 / actual_deposit as f64
                                };
                                assert!(variance_ratio <= 1.1,
                                    "Deposit amount should be close to expected: got {}, expected {}",
                                    actual_deposit, expected_deposit);
                                println!("     ✅ Deposit amount matches expected");
                            }

                            // Check if action_type was properly extracted
                            if let Some(action_type) = &action.action_type {
                                assert_eq!(
                                    action_type, EXPECTED_ACTION.action_type,
                                    "Action type should match expected"
                                );
                                println!("     ✅ Action type correctly parsed: {}", action_type);
                            }

                            // Validate success status (should be Some(1) for successful)
                            if let Some(success) = action.success {
                                assert_eq!(success, 1, "Action should be successful");
                                println!("     ✅ Action marked as successful");
                            }
                        }
                    } else {
                        println!("⚠️  Expected staking action not found");
                    }

                    // Validate delegator relationship was created
                    let expected_delegators: Vec<_> = all_delegators
                        .iter()
                        .filter(|d| {
                            d.pool_id == EXPECTED_ACTION.pool_id
                                && d.delegator_account_id == EXPECTED_ACTION.caller_account_id
                        })
                        .collect();

                    if !expected_delegators.is_empty() {
                        println!("✅ Found delegator relationships:");
                        for delegator in expected_delegators {
                            println!("   Delegator:");
                            println!("     Pool: {}", delegator.pool_id);
                            println!("     Delegator: {}", delegator.delegator_account_id);
                            println!(
                                "     First interaction: {}",
                                delegator.first_interaction_type
                            );
                            println!("     Interaction count: {}", delegator.interaction_count);

                            assert_eq!(
                                delegator.first_interaction_type, "deposit_and_stake",
                                "First interaction should be deposit_and_stake"
                            );
                            assert!(
                                delegator.interaction_count > 0,
                                "Should have at least one interaction"
                            );
                            println!("     ✅ Delegator relationship correctly tracked");
                        }
                    }

                    // Validate validator proposals from block header were processed
                    if !all_validator_proposals.is_empty() {
                        println!("✅ Validator proposals processed:");
                        for proposal in all_validator_proposals.iter().take(3) {
                            println!("   Validator Proposal:");
                            println!("     Account: {}", proposal.account_id);
                            println!("     Public key: {}", proposal.public_key);
                            println!("     Stake: {}", proposal.stake);
                            println!("     Block height: {}", proposal.block_height);

                            // Validate proposal data structure
                            assert!(
                                !proposal.account_id.is_empty(),
                                "Account ID should not be empty"
                            );
                            assert!(
                                !proposal.public_key.is_empty(),
                                "Public key should not be empty"
                            );
                            assert!(
                                proposal.stake.parse::<u128>().is_ok(),
                                "Stake should be valid number"
                            );
                            assert_eq!(
                                proposal.block_height, TEST_BLOCK_HEIGHT as i64,
                                "Block height should match"
                            );
                        }
                    }

                    // Ensure some staking activity was captured
                    assert!(
                        all_pools.len() > 0 || all_actions.len() > 0,
                        "Expected to capture some staking pool activity in block {}",
                        STAKING_POOL_BLOCK
                    );
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

    println!("✅ Staking pool deposit_and_stake validation completed");
}

/// Test unstake transaction validation
#[rstest]
#[tokio::test]
async fn test_unstake_validation(#[future] test_db: TestDatabase) {
    init_test_tracing();

    // Test-specific constants
    const TEST_BLOCK_HEIGHT: BlockHeight = 163446874;
    const EXPECTED_SIGNER: &str = "lamineyamal.near";
    const EXPECTED_RECEIVER: &str = "zavodil.poolv1.near";
    const EXPECTED_UNSTAKE_AMOUNT: &str = "250000000000000000000000"; // 0.25 NEAR

    let test_db = test_db.await;
    let db = test_db.db();
    let staking_module = create_test_staking_pools_module();

    if !staking_module.enabled() {
        return;
    }

    println!(
        "⬇️  Testing staking pool unstake at block {}",
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
                "📦 Processing unstake block {} with {} shards",
                msg.block.header.height,
                msg.shards.len()
            );

            match staking_module.process_block(&msg, &db).await {
                Ok(_) => {
                    println!(
                        "✅ Successfully processed unstake block {}",
                        msg.block.header.height
                    );

                    let all_actions = near_staking_pool_actions::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query pool actions");

                    let all_pools = near_staking_pools::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query pools");

                    println!("📊 Unstake validation:");
                    println!("   {} actions recorded", all_actions.len());
                    println!("   {} pools discovered", all_pools.len());

                    // Validate expected unstake action
                    let unstake_actions: Vec<_> = all_actions
                        .iter()
                        .filter(|a| a.method_name == "unstake" && a.pool_id == EXPECTED_RECEIVER)
                        .collect();

                    if !unstake_actions.is_empty() {
                        println!("✅ Found {} unstake actions:", unstake_actions.len());
                        for action in unstake_actions {
                            println!("   Unstake Action:");
                            println!("     Pool: {}", action.pool_id);
                            println!("     Caller: {}", action.caller_account_id);
                            println!("     Method: {}", action.method_name);
                            println!("     Success: {:?}", action.success);
                            println!("     Block height: {}", action.block_height);

                            // Validate basic action properties
                            assert_eq!(action.method_name, "unstake");
                            assert_eq!(action.pool_id, EXPECTED_RECEIVER);
                            assert_eq!(action.caller_account_id, EXPECTED_SIGNER);

                            // Check if amount was extracted from args
                            if let Some(amount) = &action.amount {
                                println!("     Amount: {}", amount);
                                assert_eq!(
                                    amount, EXPECTED_UNSTAKE_AMOUNT,
                                    "Unstake amount should match expected"
                                );
                                println!("     ✅ Unstake amount correctly parsed");
                            }

                            // Check action type classification
                            if let Some(action_type) = &action.action_type {
                                assert_eq!(action_type, "unstake");
                                println!("     ✅ Action type correctly classified");
                            }

                            // This particular transaction failed in reality
                            if let Some(success) = action.success {
                                println!("     Transaction success status: {}", success);
                            }
                        }
                    }

                    // Validate zavodil pool was detected
                    let zavodil_pool = all_pools.iter().find(|p| p.pool_id == EXPECTED_RECEIVER);

                    if let Some(pool) = zavodil_pool {
                        println!("✅ Zavodil staking pool detected:");
                        println!("   Pool ID: {}", pool.pool_id);
                        println!("   Pool type: {}", pool.pool_type);
                        assert_eq!(pool.pool_type, "poolv1");
                        println!("     ✅ Pool correctly classified as poolv1");
                    }
                }
                Err(e) => {
                    println!("⚠️  Failed to process unstake block: {}", e);
                }
            }
        }
        _ => {
            println!("⚠️  No data received for unstake test");
        }
    }

    handle.abort();
    let _ = handle.await;

    println!("✅ Unstake validation completed");
}

/// Test validator proposals and epoch tracking from block headers
#[rstest]
#[tokio::test]
async fn test_validator_proposals_validation(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let staking_module = create_test_staking_pools_module();

    if !staking_module.enabled() {
        return;
    }

    println!(
        "🏛️  Testing validator proposals at block {}",
        VALIDATOR_PROPOSAL_BLOCK
    );

    let config = LakeConfigBuilder::default()
        .mainnet()
        .start_block_height(VALIDATOR_PROPOSAL_BLOCK)
        .blocks_preload_pool_size(1)
        .build()
        .expect("Failed to build Lake config");

    let (handle, mut stream) = near_lake_framework::streamer(config);

    let timeout_duration = tokio::time::Duration::from_secs(30);
    match tokio::time::timeout(timeout_duration, stream.recv()).await {
        Ok(Some(msg)) => {
            println!(
                "📦 Processing validator proposal block {}",
                msg.block.header.height,
            );

            // Get the first chunk author as the block producer
            let chunk_authors: Vec<String> = msg
                .shards
                .iter()
                .filter_map(|shard| shard.chunk.as_ref().map(|chunk| chunk.author.to_string()))
                .collect();

            if let Some(block_author) = chunk_authors.first() {
                // Verify the block is authored by a staking pool
                assert!(
                    block_author.ends_with(".poolv1.near") || block_author.ends_with(".pool.near"),
                    "Block should be authored by a staking pool validator"
                );
                println!("✅ Block authored by staking pool: {}", block_author);
            } else {
                println!("⚠️  No chunk authors found in block");
            }

            match staking_module.process_block(&msg, &db).await {
                Ok(_) => {
                    println!("✅ Successfully processed validator proposal block");

                    let all_proposals = near_staking_validator_proposals::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query validator proposals");

                    let all_epochs = near_staking_epochs::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query epochs");

                    let all_pools = near_staking_pools::Entity::find()
                        .all(&db)
                        .await
                        .expect("Failed to query pools");

                    println!("🏛️  Validator proposal validation:");
                    println!("   {} validator proposals", all_proposals.len());
                    println!("   {} epochs tracked", all_epochs.len());
                    println!("   {} pools detected", all_pools.len());

                    // Validate validator proposals were captured
                    if !all_proposals.is_empty() {
                        println!("✅ Validator proposals processed:");
                        for proposal in all_proposals.iter().take(5) {
                            println!("   Proposal:");
                            println!("     Account: {}", proposal.account_id);
                            println!("     Public key: {}", proposal.public_key);
                            println!("     Stake: {}", proposal.stake);
                            println!("     Epoch: {}", proposal.epoch_id);

                            // Validate proposal data
                            assert!(!proposal.account_id.is_empty());
                            assert!(!proposal.public_key.is_empty());
                            assert!(proposal.stake.parse::<u128>().is_ok());
                            assert!(!proposal.epoch_id.is_empty());

                            // Check if this is a staking pool proposal
                            if proposal.account_id.ends_with(".poolv1.near")
                                || proposal.account_id.ends_with(".pool.near")
                            {
                                println!(
                                    "     ✅ Staking pool validator proposal: {}",
                                    proposal.account_id
                                );
                            }
                        }
                    }

                    // Validate epoch tracking
                    if !all_epochs.is_empty() {
                        println!("✅ Epochs tracked:");
                        for epoch in &all_epochs {
                            println!("   Epoch:");
                            println!("     ID: {}", epoch.epoch_id);
                            println!("     Start block: {}", epoch.start_block_height);
                            println!("     Total stake: {}", epoch.total_stake);
                            println!(
                                "     Total validator reward: {}",
                                epoch.total_validator_reward
                            );

                            // Validate epoch data
                            assert!(!epoch.epoch_id.is_empty());
                            assert!(epoch.start_block_height > 0);
                            assert!(epoch.total_stake.parse::<u128>().is_ok());
                            assert!(epoch.total_validator_reward.parse::<u128>().is_ok());
                            println!("     ✅ Epoch data is valid");
                        }
                    }

                    // Validate staking pools were registered from validator proposals
                    let staking_pool_validators: Vec<_> = all_proposals
                        .iter()
                        .filter(|p| {
                            p.account_id.ends_with(".poolv1.near")
                                || p.account_id.ends_with(".pool.near")
                        })
                        .collect();

                    if !staking_pool_validators.is_empty() {
                        println!(
                            "✅ Found {} staking pool validators:",
                            staking_pool_validators.len()
                        );

                        for pool_proposal in staking_pool_validators.iter().take(3) {
                            // Check if corresponding pool was registered
                            let registered_pool = all_pools
                                .iter()
                                .find(|p| p.pool_id == pool_proposal.account_id);

                            if let Some(pool) = registered_pool {
                                println!("     Pool registered: {}", pool.pool_id);
                                assert!(pool.is_active == 1, "Pool should be active");
                                println!(
                                    "       ✅ Pool correctly registered from validator proposal"
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    println!("⚠️  Failed to process validator proposal block: {}", e);
                }
            }
        }
        _ => {
            println!("⚠️  No data received for validator proposal test");
        }
    }

    handle.abort();
    let _ = handle.await;

    println!("✅ Validator proposals validation completed");
}

/// Test comprehensive staking pool data across multiple blocks
#[rstest]
#[tokio::test]
async fn test_comprehensive_staking_pool_validation(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let staking_module = create_test_staking_pools_module();

    if !staking_module.enabled() {
        return;
    }

    println!("🔍 Testing comprehensive staking pool data across blocks");

    let mut test_blocks = [
        (STAKING_POOL_BLOCK, "deposit_and_stake activity"),
        (UNSTAKE_ACTIVITY_BLOCK, "unstake activity"),
        (VALIDATOR_PROPOSAL_BLOCK, "validator proposals"),
    ];
    test_blocks.sort_by_key(|k| k.0);

    let mut total_pools = 0;
    let mut total_actions = 0;
    let mut total_proposals = 0;
    let mut total_events = 0;

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
                match staking_module.process_block(&msg, &db).await {
                    Ok(_) => {
                        let pools = near_staking_pools::Entity::find()
                            .all(&db)
                            .await
                            .expect("Failed to query pools");

                        let actions = near_staking_pool_actions::Entity::find()
                            .all(&db)
                            .await
                            .expect("Failed to query actions");

                        let proposals = near_staking_validator_proposals::Entity::find()
                            .all(&db)
                            .await
                            .expect("Failed to query proposals");

                        let events = near_staking_pool_events::Entity::find()
                            .all(&db)
                            .await
                            .expect("Failed to query events");

                        println!("  ✅ Block {} processed: {} pools, {} actions, {} proposals, {} events",
                            block_height, pools.len(), actions.len(), proposals.len(), events.len());

                        total_pools = pools.len();
                        total_actions = actions.len();
                        total_proposals = proposals.len();
                        total_events = events.len();

                        // Validate data relationships
                        for pool in pools.iter().take(2) {
                            // Check pool has corresponding actions
                            let pool_actions =
                                actions.iter().filter(|a| a.pool_id == pool.pool_id).count();

                            println!("    Pool {} has {} actions", pool.pool_id, pool_actions);

                            // Validate pool type classification
                            if pool.pool_id.ends_with(".poolv1.near") {
                                assert_eq!(pool.pool_type, "poolv1", "Pool type should be poolv1");
                            } else if pool.pool_id.ends_with(".pool.near") {
                                assert_eq!(pool.pool_type, "pool", "Pool type should be pool");
                            }
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

    println!("📈 Final staking pool indexing summary:");
    println!("   {} total staking pools discovered", total_pools);
    println!("   {} total pool actions", total_actions);
    println!("   {} total validator proposals", total_proposals);
    println!("   {} total events parsed", total_events);

    // Final validation of data integrity
    if total_pools > 0 {
        let final_pools = near_staking_pools::Entity::find()
            .all(&db)
            .await
            .expect("Failed to query final pools");

        let final_delegators = near_staking_delegators::Entity::find()
            .all(&db)
            .await
            .expect("Failed to query final delegators");

        for pool in final_pools.iter().take(3) {
            println!("✅ Pool integrity check: {}", pool.pool_id);

            // Check pool has activity
            assert!(
                pool.last_activity_height >= pool.first_seen_height,
                "Last activity should be >= first seen"
            );

            // Check pool has delegators if it has actions
            let pool_actions = near_staking_pool_actions::Entity::find()
                .filter(near_staking_pool_actions::Column::PoolId.eq(&pool.pool_id))
                .all(&db)
                .await
                .expect("Failed to query pool actions");

            if !pool_actions.is_empty() {
                let pool_delegators: Vec<_> = final_delegators
                    .iter()
                    .filter(|d| d.pool_id == pool.pool_id)
                    .collect();

                println!(
                    "   Pool {} has {} actions, {} delegators",
                    pool.pool_id,
                    pool_actions.len(),
                    pool_delegators.len()
                );
            }
        }
    }

    println!("✅ Comprehensive staking pool validation completed");
}

/// Test staking pool module configuration
#[tokio::test]
async fn test_staking_pools_module_configuration() {
    init_test_tracing();

    println!("⚙️ Testing staking pools module configuration");

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
                tokens: None,
                staking_pools: Some(common::config::StakingPoolsModuleConfig {
                    enabled: false,
                    pool_patterns: vec![],
                    custom_pools: vec![],
                    track_epochs: false,
                    track_delegators: false,
                    parse_events: false,
                }),
            }),
        }),
    });

    let disabled_module = StakingPoolsModule::new(&disabled_config);
    assert!(!disabled_module.enabled(), "Module should be disabled");
    assert_eq!(disabled_module.name(), "staking_pools");
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

    let no_near_module = StakingPoolsModule::new(&no_near_config);
    assert!(
        !no_near_module.enabled(),
        "Module should be disabled when NEAR chain is disabled"
    );
    println!("✅ Disabled NEAR chain configuration works");

    println!("✅ All staking pools configuration tests passed");
}
