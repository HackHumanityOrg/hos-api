use near_indexer::modules::network::NetworkModule;
use near_indexer::IndexerModule;
use near_lake_framework::near_indexer_primitives as nip;
use near_primitives::types::StateRoot;
use nip::views as v;
use sea_orm::{EntityTrait, PaginatorTrait};
use std::str::FromStr;
use test_support::TestDatabase;

mod testutil;
use testutil::make_block;

fn cfg_enable_network() -> common::config::AppConfig {
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
            concurrency: Some(1),
            region: None,
            modules: Some(common::config::NearModulesConfig {
                accounts: None,
                tokens: None,
                validators: None,
                network: Some(common::config::NetworkModuleConfig { enabled: true }),
            }),
        }),
    })
}

fn create_shard_with_chunk(shard_id: u64, chunk_hash: &str, producer: &str) -> nip::IndexerShard {
    let chunk_header = v::ChunkHeaderView {
        chunk_hash: nip::CryptoHash::from_str(chunk_hash).unwrap(),
        prev_block_hash: nip::CryptoHash::default(),
        outcome_root: nip::CryptoHash::default(),
        prev_state_root: StateRoot::default(),
        encoded_merkle_root: nip::CryptoHash::default(),
        encoded_length: 0,
        height_created: 100,
        height_included: 100,
        shard_id: shard_id.into(),
        gas_used: 1_000_000,
        gas_limit: 10_000_000,
        validator_reward: 1000000000000000000000u128,
        balance_burnt: 0,
        outgoing_receipts_root: nip::CryptoHash::default(),
        tx_root: nip::CryptoHash::default(),
        validator_proposals: vec![],
        signature: near_crypto::Signature::default(),
        bandwidth_requests: None,
        congestion_info: Default::default(),
        rent_paid: 0,
    };

    let chunk = nip::IndexerChunkView {
        author: producer.parse().unwrap(),
        header: chunk_header,
        transactions: vec![], // Empty for simplicity
        receipts: vec![],
    };

    nip::IndexerShard {
        shard_id: shard_id.into(),
        chunk: Some(chunk),
        receipt_execution_outcomes: vec![],
        state_changes: vec![],
    }
}

#[tokio::test]
async fn network_module_records_comprehensive_metrics() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    let module = NetworkModule::new(&cfg_enable_network());
    module.initialize(&db).await.unwrap();

    // Create a block with multiple shards
    let mut msg = make_block(100, 1640995200000000000); // 2022-01-01 timestamp

    // Add shards with different states - some with chunks, some without
    msg.shards
        .push(create_shard_with_chunk(0, "chunk0hash", "validator0.near"));
    msg.shards
        .push(create_shard_with_chunk(1, "chunk1hash", "validator1.near"));

    // Add a shard without a chunk (missing chunk)
    msg.shards.push(nip::IndexerShard {
        shard_id: 2u64.into(),
        chunk: None,
        receipt_execution_outcomes: vec![],
        state_changes: vec![],
    });

    // Process the block
    module.process_block(&msg, &db).await.unwrap();

    // Verify network state was recorded
    let network_state_count = entities::near_network_state::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(network_state_count, 1);

    // Verify epoch was recorded
    let epochs_count = entities::near_epochs::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(epochs_count, 1);

    // Verify chunk data was recorded (only for shards with chunks)
    let chunks_count = entities::near_chunks::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(chunks_count, 2); // Only shards 0 and 1 had chunks

    // Verify gas price dynamics were recorded
    let gas_prices_count = entities::near_gas_prices::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(gas_prices_count, 1);

    // Verify block metrics were recorded
    let block_metrics_count = entities::near_block_metrics::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(block_metrics_count, 1);

    // Check specific chunk data
    let chunks: Vec<entities::near_chunks::Model> = entities::near_chunks::Entity::find()
        .all(&db)
        .await
        .unwrap();

    assert_eq!(chunks.len(), 2);

    // Verify shard 0 chunk
    let chunk_0 = chunks.iter().find(|c| c.shard_id == 0).unwrap();
    assert_eq!(chunk_0.chunk_hash, "chunk0hash");
    assert_eq!(chunk_0.chunk_producer.as_ref().unwrap(), "validator0.near");
    assert_eq!(chunk_0.gas_used, 1_000_000);
    assert_eq!(chunk_0.gas_limit, 10_000_000);

    // Verify block metrics show correct chunk counts
    let block_metrics = entities::near_block_metrics::Entity::find()
        .one(&db)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(block_metrics.chunks_produced, 2);
    assert_eq!(block_metrics.chunks_missing, 1); // Shard 2 had no chunk
    assert_eq!(block_metrics.total_gas_used, 2_000_000); // 1M + 1M from two chunks
    assert_eq!(block_metrics.total_gas_limit, 20_000_000); // 10M + 10M from two chunks

    // Verify gas price dynamics
    let gas_price_record = entities::near_gas_prices::Entity::find()
        .one(&db)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(gas_price_record.gas_used, 2_000_000);
    assert_eq!(gas_price_record.gas_limit, 20_000_000);
    // Utilization should be 0.1 (2M/20M)
    assert!(gas_price_record
        .utilization_ratio
        .unwrap()
        .to_string()
        .starts_with("0.1"));
}

#[tokio::test]
async fn network_module_tracks_epoch_transitions() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    let module = NetworkModule::new(&cfg_enable_network());
    module.initialize(&db).await.unwrap();

    // First block in epoch 1
    let mut msg1 = make_block(100, 1640995200000000000);
    msg1.block.header.epoch_id = "epoch1".parse().unwrap();
    msg1.shards.clear(); // No shards needed for this test

    module.process_block(&msg1, &db).await.unwrap();

    // Second block in same epoch
    let mut msg2 = make_block(101, 1640995201000000000);
    msg2.block.header.epoch_id = "epoch1".parse().unwrap();
    msg2.shards.clear();

    module.process_block(&msg2, &db).await.unwrap();

    // Third block in new epoch
    let mut msg3 = make_block(102, 1640995202000000000);
    msg3.block.header.epoch_id = "epoch2".parse().unwrap();
    msg3.shards.clear();

    module.process_block(&msg3, &db).await.unwrap();

    // Should have 2 epochs recorded
    let epochs: Vec<entities::near_epochs::Model> = entities::near_epochs::Entity::find()
        .all(&db)
        .await
        .unwrap();

    assert_eq!(epochs.len(), 2);

    // First epoch should be closed
    let epoch1 = epochs.iter().find(|e| e.epoch_id == "epoch1").unwrap();
    assert_eq!(epoch1.start_height, 100);
    assert_eq!(epoch1.end_height, Some(101)); // Closed at block 101

    // Second epoch should be open
    let epoch2 = epochs.iter().find(|e| e.epoch_id == "epoch2").unwrap();
    assert_eq!(epoch2.start_height, 102);
    assert_eq!(epoch2.end_height, None); // Still open
}

#[tokio::test]
async fn network_module_calculates_gas_price_dynamics() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    let module = NetworkModule::new(&cfg_enable_network());
    module.initialize(&db).await.unwrap();

    // First block with low utilization
    let mut msg1 = make_block(100, 1640995200000000000);
    msg1.shards
        .push(create_shard_with_chunk(0, "chunk1", "validator.near"));
    // Modify gas usage for testing
    if let Some(ref mut chunk) = msg1.shards[0].chunk {
        chunk.header.gas_used = 1_000_000; // 10% utilization
        chunk.header.gas_limit = 10_000_000;
    }

    module.process_block(&msg1, &db).await.unwrap();

    // Second block with high utilization
    let mut msg2 = make_block(101, 1640995201000000000);
    msg2.shards
        .push(create_shard_with_chunk(0, "chunk2", "validator.near"));
    // High utilization should cause price increase
    if let Some(ref mut chunk) = msg2.shards[0].chunk {
        chunk.header.gas_used = 8_000_000; // 80% utilization
        chunk.header.gas_limit = 10_000_000;
    }

    module.process_block(&msg2, &db).await.unwrap();

    // Check gas price records
    let gas_prices: Vec<entities::near_gas_prices::Model> =
        entities::near_gas_prices::Entity::find()
            .all(&db)
            .await
            .unwrap();

    assert_eq!(gas_prices.len(), 2);

    // First block should have low utilization
    let price1 = gas_prices.iter().find(|p| p.block_height == 100).unwrap();
    assert_eq!(price1.gas_used, 1_000_000);
    assert_eq!(price1.gas_limit, 10_000_000);
    assert!(price1
        .utilization_ratio
        .unwrap()
        .to_string()
        .starts_with("0.1"));

    // Second block should have high utilization
    let price2 = gas_prices.iter().find(|p| p.block_height == 101).unwrap();
    assert_eq!(price2.gas_used, 8_000_000);
    assert_eq!(price2.gas_limit, 10_000_000);
    assert!(price2
        .utilization_ratio
        .unwrap()
        .to_string()
        .starts_with("0.8"));

    // Second block should reference first block's gas price
    assert!(price2.prev_gas_price.is_some());
}

#[tokio::test]
async fn network_module_handles_empty_blocks() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    let module = NetworkModule::new(&cfg_enable_network());
    module.initialize(&db).await.unwrap();

    // Block with no shards/chunks
    let mut msg = make_block(100, 1640995200000000000);
    msg.shards.clear();

    module.process_block(&msg, &db).await.unwrap();

    // Should still record basic network state
    let network_state_count = entities::near_network_state::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(network_state_count, 1);

    // Should record epoch
    let epochs_count = entities::near_epochs::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(epochs_count, 1);

    // Should record gas prices (even with 0 usage)
    let gas_prices_count = entities::near_gas_prices::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(gas_prices_count, 1);

    // Should record block metrics
    let block_metrics = entities::near_block_metrics::Entity::find()
        .one(&db)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(block_metrics.chunks_produced, 0);
    assert_eq!(block_metrics.chunks_missing, 0);
    assert_eq!(block_metrics.total_gas_used, 0);
    assert_eq!(block_metrics.total_gas_limit, 0);

    // No chunks should be recorded
    let chunks_count = entities::near_chunks::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(chunks_count, 0);
}
