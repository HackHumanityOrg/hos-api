use entities::*;
use near_indexer::modules::validators::ValidatorsModule;
use near_indexer::IndexerModule;
use near_lake_framework::near_indexer_primitives as nip;
use sea_orm::EntityTrait;
use test_support::TestDatabase;

mod testutil;
use testutil::{make_block, make_chunk_with_producer};

fn cfg_enable_validators() -> common::config::AppConfig {
    common::config::AppConfig::Indexer(common::config::IndexerConfig {
        database: common::config::DatabaseConfig {
            url: String::new(),
            max_connections: None,
            min_connections: None,
            connection_timeout: None,
            acquire_timeout: None,
            idle_timeout: None,
            max_lifetime: None,
        },
        logging: common::config::LoggingConfig {
            level: "debug".into(),
            format: None,
        },
        telemetry: common::config::TelemetryConfig {
            enabled: false,
            service_version: "test".into(),
            environment: "test".into(),
            enable_metrics: false,
            enable_traces: false,
            enable_logs: false,
            batch_size: 0,
            export_timeout_sec: 0,
            export_interval_sec: 0,
            resource_attributes: Default::default(),
        },
        discourse: None,
        near: Some(common::config::NearLakeConfig {
            chain: "mainnet".into(),
            start_height: 0,
            concurrency: Some(2),
            region: None,
            modules: Some(common::config::NearModulesConfig {
                accounts: Some(common::config::AccountsModuleConfig {
                    enabled: true,
                    track_implicit: true,
                    track_eth_implicit: true,
                }),
                tokens: None,
                validators: Some(common::config::ValidatorsModuleConfig { 
                    enabled: true,
                    rpc_updater: None,
                }),
                network: Some(common::config::NetworkModuleConfig { enabled: true }),
            }),
        }),
    })
}

#[tokio::test]
async fn test_block_validator_rewards_persistence() {
    let test_db = TestDatabase::new().await;
    let config = cfg_enable_validators();
    let validators_module = ValidatorsModule::new(&config);

    let mut block = make_block(12345, 1000000000);
    block.block.header.validator_reward = 1_000_000_000_000_000_000; // 1 NEAR
    block.block.author = "block-producer.near".parse().unwrap();

    validators_module
        .process_block(&block, &test_db.db())
        .await
        .unwrap();

    // Check that validator rewards were recorded
    let rewards = near_validator_rewards::Entity::find()
        .all(&test_db.db())
        .await
        .unwrap();

    assert_eq!(rewards.len(), 2); // Total and block producer rewards

    // Check total reward entry
    let total_reward = rewards
        .iter()
        .find(|r| r.validator_id == "*total*")
        .unwrap();
    assert_eq!(total_reward.source, "block");
    assert_eq!(
        total_reward.reward_amount.to_string(),
        "1000000000000000000"
    );
    assert_eq!(total_reward.block_height, 12345);
    assert!(total_reward.shard_id.is_none());

    // Check block producer reward entry
    let producer_reward = rewards
        .iter()
        .find(|r| r.validator_id == "block-producer.near")
        .unwrap();
    assert_eq!(producer_reward.source, "block");
    assert_eq!(
        producer_reward.reward_amount.to_string(),
        "1000000000000000000"
    );
    assert_eq!(producer_reward.block_height, 12345);
    assert!(producer_reward.shard_id.is_none());
}

#[tokio::test]
async fn test_chunk_validator_rewards_persistence() {
    let test_db = TestDatabase::new().await;
    let config = cfg_enable_validators();
    let validators_module = ValidatorsModule::new(&config);

    let mut block = make_block(12345, 1000000000);

    // Add chunk with reward
    let chunk_with_reward =
        make_chunk_with_producer(12345, 0, "test-producer.near", 500_000_000_000_000_000); // 0.5 NEAR
    block.shards = vec![nip::IndexerShard {
        shard_id: 0u64.into(),
        chunk: Some(chunk_with_reward),
        receipt_execution_outcomes: vec![],
        state_changes: vec![],
    }];

    validators_module
        .process_block(&block, &test_db.db())
        .await
        .unwrap();

    // Check that chunk validator rewards were recorded
    let rewards = near_validator_rewards::Entity::find()
        .all(&test_db.db())
        .await
        .unwrap();

    // Should have block rewards (total + producer) + chunk reward
    let chunk_rewards: Vec<_> = rewards.iter().filter(|r| r.source == "chunk").collect();
    assert_eq!(chunk_rewards.len(), 1);

    let chunk_reward = chunk_rewards[0];
    assert_eq!(chunk_reward.validator_id, "test-producer.near");
    assert_eq!(chunk_reward.reward_amount.to_string(), "500000000000000000");
    assert_eq!(chunk_reward.block_height, 12345);
    assert_eq!(chunk_reward.shard_id, Some(0));
}

#[tokio::test]
async fn test_validator_approvals_persistence() {
    let test_db = TestDatabase::new().await;
    let config = cfg_enable_validators();
    let validators_module = ValidatorsModule::new(&config);

    let mut block = make_block(12345, 1000000000);

    // Add some approvals (3 out of 5 validators approved)
    block.block.header.approvals = vec![
        Some(Box::new(near_crypto::Signature::empty(
            near_crypto::KeyType::ED25519,
        ))),
        None,
        Some(Box::new(near_crypto::Signature::empty(
            near_crypto::KeyType::ED25519,
        ))),
        Some(Box::new(near_crypto::Signature::empty(
            near_crypto::KeyType::ED25519,
        ))),
        None,
    ];

    validators_module
        .process_block(&block, &test_db.db())
        .await
        .unwrap();

    // Check validator approvals
    let approvals = near_validator_approvals::Entity::find()
        .all(&test_db.db())
        .await
        .unwrap();

    assert_eq!(approvals.len(), 1);

    let approval = &approvals[0];
    assert_eq!(approval.block_height, 12345);
    assert_eq!(approval.total_approvals, 3);
    assert_eq!(approval.total_slots, 5);
}

#[tokio::test]
async fn test_block_producer_persistence() {
    let test_db = TestDatabase::new().await;
    let config = cfg_enable_validators();
    let validators_module = ValidatorsModule::new(&config);

    let mut block = make_block(12345, 1000000000);
    block.block.author = "block-producer.near".parse().unwrap();
    block.block.header.gas_price = 1_000_000_000;

    validators_module
        .process_block(&block, &test_db.db())
        .await
        .unwrap();

    // Check block producer
    let producers = near_block_producers::Entity::find()
        .all(&test_db.db())
        .await
        .unwrap();

    assert_eq!(producers.len(), 1);

    let producer = &producers[0];
    assert_eq!(producer.producer_id, "block-producer.near");
    assert_eq!(producer.block_height, 12345);
    assert_eq!(producer.gas_price.to_string(), "1000000000");
}

#[tokio::test]
async fn test_chunk_producer_persistence() {
    let test_db = TestDatabase::new().await;
    let config = cfg_enable_validators();
    let validators_module = ValidatorsModule::new(&config);

    let mut block = make_block(12345, 1000000000);

    // Add chunks for multiple shards
    let chunk1 = make_chunk_with_producer(12345, 0, "producer1.near", 100_000_000);
    let chunk2 = make_chunk_with_producer(12345, 1, "producer2.near", 200_000_000);

    block.shards = vec![
        nip::IndexerShard {
            shard_id: 0u64.into(),
            chunk: Some(chunk1),
            receipt_execution_outcomes: vec![],
            state_changes: vec![],
        },
        nip::IndexerShard {
            shard_id: 1u64.into(),
            chunk: Some(chunk2),
            receipt_execution_outcomes: vec![],
            state_changes: vec![],
        },
    ];

    validators_module
        .process_block(&block, &test_db.db())
        .await
        .unwrap();

    // Check chunk producers
    let producers = near_chunk_producers::Entity::find()
        .all(&test_db.db())
        .await
        .unwrap();

    assert_eq!(producers.len(), 2);

    let shard0_producer = producers.iter().find(|p| p.shard_id == 0).unwrap();
    assert_eq!(shard0_producer.producer_id, "producer1.near");
    assert_eq!(shard0_producer.block_height, 12345);

    let shard1_producer = producers.iter().find(|p| p.shard_id == 1).unwrap();
    assert_eq!(shard1_producer.producer_id, "producer2.near");
    assert_eq!(shard1_producer.block_height, 12345);
}

#[tokio::test]
async fn test_no_rewards_for_zero_amounts() {
    let test_db = TestDatabase::new().await;
    let config = cfg_enable_validators();
    let validators_module = ValidatorsModule::new(&config);

    let mut block = make_block(12345, 1000000000);
    block.block.header.validator_reward = 0; // No block reward

    // Add chunk with zero reward
    let chunk_no_reward = make_chunk_with_producer(12345, 0, "test-producer.near", 0);
    block.shards = vec![nip::IndexerShard {
        shard_id: 0u64.into(),
        chunk: Some(chunk_no_reward),
        receipt_execution_outcomes: vec![],
        state_changes: vec![],
    }];

    validators_module
        .process_block(&block, &test_db.db())
        .await
        .unwrap();

    // Check that no rewards were recorded
    let rewards = near_validator_rewards::Entity::find()
        .all(&test_db.db())
        .await
        .unwrap();

    assert_eq!(rewards.len(), 0);
}

#[tokio::test]
async fn test_validators_persistence_multiple_blocks() {
    let test_db = TestDatabase::new().await;
    let config = cfg_enable_validators();
    let validators_module = ValidatorsModule::new(&config);

    // Process first block
    let mut block1 = make_block(12345, 1000000000);
    block1.block.header.validator_reward = 1_000_000_000;
    block1.block.author = "producer1.near".parse().unwrap();

    validators_module
        .process_block(&block1, &test_db.db())
        .await
        .unwrap();

    // Process second block
    let mut block2 = make_block(12346, 1000000001);
    block2.block.header.validator_reward = 2_000_000_000;
    block2.block.author = "producer2.near".parse().unwrap();

    validators_module
        .process_block(&block2, &test_db.db())
        .await
        .unwrap();

    // Check that all rewards were recorded
    let rewards = near_validator_rewards::Entity::find()
        .all(&test_db.db())
        .await
        .unwrap();

    // Should have 4 entries: 2 blocks * (total + producer reward)
    assert_eq!(rewards.len(), 4);

    // Check block producers
    let producers = near_block_producers::Entity::find()
        .all(&test_db.db())
        .await
        .unwrap();

    assert_eq!(producers.len(), 2);

    let producer1 = producers
        .iter()
        .find(|p| p.producer_id == "producer1.near")
        .unwrap();
    assert_eq!(producer1.block_height, 12345);

    let producer2 = producers
        .iter()
        .find(|p| p.producer_id == "producer2.near")
        .unwrap();
    assert_eq!(producer2.block_height, 12346);
}
