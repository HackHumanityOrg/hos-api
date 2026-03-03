//! Integration tests for the raw NEAR module using real fixtures.

mod support;

use anyhow::Result;
use common::config::{
    AppConfig, DatabaseConfig, IndexerConfig, LoggingConfig, NearLakeConfig, NearModuleStreamMode,
    NearModulesConfig, NearStreamProvider, RawModuleConfig, TelemetryConfig,
};
use near_indexer::modules::raw::RawModule;
use near_indexer::IndexerModule;
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use support::{init_test_tracing, TestDatabase};

#[derive(Debug, Default)]
struct RawFixtureExpectations {
    chunk_hashes: HashSet<String>,
    transaction_shards: HashMap<String, i64>,
    transaction_signers: HashMap<String, String>,
    transaction_receivers: HashMap<String, String>,
    transaction_nonces: HashMap<String, i64>,
    receipt_shards: HashMap<String, i64>,
    receipt_predecessors: HashMap<String, String>,
    receipt_receivers: HashMap<String, String>,
    outcome_shards: HashMap<String, i64>,
    outcome_receipt_ids: HashMap<String, Option<String>>,
    local_receipt_ids: HashSet<String>,
    state_change_rows: i64,
}

fn load_fixture(name: &str) -> StreamerMessage {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/venear_governance")
        .join(format!("{name}.json"));

    let bytes = std::fs::read(&path)
        .unwrap_or_else(|err| panic!("Failed to read fixture {}: {err}", path.display()));

    serde_json::from_slice(&bytes)
        .unwrap_or_else(|err| panic!("Failed to deserialize fixture {}: {err}", path.display()))
}

fn build_test_config() -> AppConfig {
    let db = DatabaseConfig {
        url: "postgres://unused".to_string(),
        ..DatabaseConfig::default()
    };

    let cfg = IndexerConfig {
        database: db,
        logging: LoggingConfig::default(),
        telemetry: TelemetryConfig::default(),
        discourse: None,
        telegram: None,
        near: Some(NearLakeConfig {
            provider: NearStreamProvider::Lake,
            chain: "mainnet".to_string(),
            start_height: 0,
            concurrency: Some(1),
            preload_pool_size: Some(1),
            fastnear_api_key: None,
            fastnear_num_threads: None,
            modules: Some(NearModulesConfig {
                raw: Some(RawModuleConfig {
                    enabled: true,
                    stream_mode: NearModuleStreamMode::Shared,
                    start_height: None,
                }),
                accounts: None,
                tokens: None,
                staking_pools: None,
                venear_governance: None,
            }),
        }),
    };

    AppConfig::Indexer(Box::new(cfg))
}

fn collect_expectations(msg: &StreamerMessage) -> RawFixtureExpectations {
    let mut expectations = RawFixtureExpectations::default();

    for shard in &msg.shards {
        let shard_id = Into::<u64>::into(shard.shard_id) as i64;

        if !shard.state_changes.is_empty() {
            expectations.state_change_rows += 1;
        }

        if let Some(chunk) = &shard.chunk {
            expectations
                .chunk_hashes
                .insert(chunk.header.chunk_hash.to_string());

            for tx_with_outcome in &chunk.transactions {
                let tx = &tx_with_outcome.transaction;
                expectations
                    .transaction_shards
                    .entry(tx.hash.to_string())
                    .or_insert(shard_id);
                expectations
                    .transaction_signers
                    .entry(tx.hash.to_string())
                    .or_insert_with(|| tx.signer_id.to_string());
                expectations
                    .transaction_receivers
                    .entry(tx.hash.to_string())
                    .or_insert_with(|| tx.receiver_id.to_string());
                expectations
                    .transaction_nonces
                    .entry(tx.hash.to_string())
                    .or_insert(tx.nonce as i64);

                if let Some(receipt) = &tx_with_outcome.outcome.receipt {
                    expectations
                        .receipt_shards
                        .entry(receipt.receipt_id.to_string())
                        .or_insert(shard_id);
                    expectations
                        .receipt_predecessors
                        .entry(receipt.receipt_id.to_string())
                        .or_insert_with(|| receipt.predecessor_id.to_string());
                    expectations
                        .receipt_receivers
                        .entry(receipt.receipt_id.to_string())
                        .or_insert_with(|| receipt.receiver_id.to_string());
                }

                expectations
                    .outcome_shards
                    .entry(tx_with_outcome.outcome.execution_outcome.id.to_string())
                    .or_insert(shard_id);
                expectations
                    .outcome_receipt_ids
                    .entry(tx_with_outcome.outcome.execution_outcome.id.to_string())
                    .or_insert_with(|| {
                        tx_with_outcome
                            .outcome
                            .receipt
                            .as_ref()
                            .map(|receipt| receipt.receipt_id.to_string())
                    });
            }

            for receipt in &chunk.receipts {
                expectations
                    .receipt_shards
                    .entry(receipt.receipt_id.to_string())
                    .or_insert(shard_id);
                expectations
                    .receipt_predecessors
                    .entry(receipt.receipt_id.to_string())
                    .or_insert_with(|| receipt.predecessor_id.to_string());
                expectations
                    .receipt_receivers
                    .entry(receipt.receipt_id.to_string())
                    .or_insert_with(|| receipt.receiver_id.to_string());
            }

            for receipt in &chunk.local_receipts {
                let receipt_id = receipt.receipt_id.to_string();
                expectations
                    .receipt_shards
                    .entry(receipt_id.clone())
                    .or_insert(shard_id);
                expectations
                    .receipt_predecessors
                    .entry(receipt_id.clone())
                    .or_insert_with(|| receipt.predecessor_id.to_string());
                expectations
                    .receipt_receivers
                    .entry(receipt_id.clone())
                    .or_insert_with(|| receipt.receiver_id.to_string());
                expectations.local_receipt_ids.insert(receipt_id);
            }
        }

        for outcome_with_receipt in &shard.receipt_execution_outcomes {
            expectations
                .receipt_shards
                .entry(outcome_with_receipt.receipt.receipt_id.to_string())
                .or_insert(shard_id);
            expectations
                .receipt_predecessors
                .entry(outcome_with_receipt.receipt.receipt_id.to_string())
                .or_insert_with(|| outcome_with_receipt.receipt.predecessor_id.to_string());
            expectations
                .receipt_receivers
                .entry(outcome_with_receipt.receipt.receipt_id.to_string())
                .or_insert_with(|| outcome_with_receipt.receipt.receiver_id.to_string());
            expectations
                .outcome_shards
                .entry(outcome_with_receipt.execution_outcome.id.to_string())
                .or_insert(shard_id);
            expectations
                .outcome_receipt_ids
                .entry(outcome_with_receipt.execution_outcome.id.to_string())
                .or_insert_with(|| Some(outcome_with_receipt.receipt.receipt_id.to_string()));
        }
    }

    expectations
}

async fn scalar_i64(db: &impl ConnectionTrait, sql: &str, values: Vec<Value>) -> Result<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            values,
        ))
        .await?
        .expect("expected one row from scalar query");
    Ok(row.try_get("", "value")?)
}

async fn scalar_string(db: &impl ConnectionTrait, sql: &str, values: Vec<Value>) -> Result<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            values,
        ))
        .await?
        .expect("expected one row from scalar query");
    Ok(row.try_get("", "value")?)
}

#[tokio::test]
async fn raw_module_stores_complete_payloads_and_shard_ids() -> Result<()> {
    init_test_tracing();

    let fixture = load_fixture("delegate_all");
    let height = fixture.block.header.height as i64;
    let expectations = collect_expectations(&fixture);
    assert!(
        !expectations.local_receipt_ids.is_empty(),
        "fixture should contain local receipts"
    );

    let db = TestDatabase::new().await;
    let module = RawModule::new(&build_test_config());

    module.initialize(db.connection.as_ref()).await?;
    module
        .process_block(&fixture, db.connection.as_ref())
        .await?;

    let block_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_blocks WHERE height = $1",
        vec![height.into()],
    )
    .await?;
    assert_eq!(block_count, 1);

    let chunk_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_chunks WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    assert_eq!(chunk_count, expectations.chunk_hashes.len() as i64);

    let tx_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_transactions WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    assert_eq!(tx_count, expectations.transaction_shards.len() as i64);

    let receipt_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipts WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    assert_eq!(receipt_count, expectations.receipt_shards.len() as i64);

    let outcome_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_execution_outcomes WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    assert_eq!(outcome_count, expectations.outcome_shards.len() as i64);

    let state_change_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_state_changes WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    assert_eq!(state_change_count, expectations.state_change_rows);

    let block_raw_nulls = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_blocks WHERE height = $1 AND raw_data IS NULL",
        vec![height.into()],
    )
    .await?;
    assert_eq!(block_raw_nulls, 0);

    let chunk_raw_nulls = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_chunks WHERE block_height = $1 AND raw_data IS NULL",
        vec![height.into()],
    )
    .await?;
    assert_eq!(chunk_raw_nulls, 0);

    let tx_raw_nulls = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_transactions WHERE block_height = $1 AND raw_data IS NULL",
        vec![height.into()],
    )
    .await?;
    assert_eq!(tx_raw_nulls, 0);

    let receipt_raw_nulls = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipts WHERE block_height = $1 AND raw_data IS NULL",
        vec![height.into()],
    )
    .await?;
    assert_eq!(receipt_raw_nulls, 0);

    let outcome_raw_nulls = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_execution_outcomes WHERE block_height = $1 AND raw_data IS NULL",
        vec![height.into()],
    )
    .await?;
    assert_eq!(outcome_raw_nulls, 0);

    for (tx_hash, expected_shard_id) in &expectations.transaction_shards {
        let shard_id = scalar_i64(
            db.connection.as_ref(),
            "SELECT shard_id::bigint AS value FROM near.near_transactions WHERE hash = $1",
            vec![tx_hash.clone().into()],
        )
        .await?;
        assert_eq!(shard_id, *expected_shard_id, "tx hash {tx_hash}");

        let signer_id = scalar_string(
            db.connection.as_ref(),
            "SELECT signer_id::text AS value FROM near.near_transactions WHERE hash = $1",
            vec![tx_hash.clone().into()],
        )
        .await?;
        assert_eq!(
            signer_id, expectations.transaction_signers[tx_hash],
            "tx signer mismatch for hash {tx_hash}"
        );

        let receiver_id = scalar_string(
            db.connection.as_ref(),
            "SELECT receiver_id::text AS value FROM near.near_transactions WHERE hash = $1",
            vec![tx_hash.clone().into()],
        )
        .await?;
        assert_eq!(
            receiver_id, expectations.transaction_receivers[tx_hash],
            "tx receiver mismatch for hash {tx_hash}"
        );

        let nonce = scalar_i64(
            db.connection.as_ref(),
            "SELECT nonce::bigint AS value FROM near.near_transactions WHERE hash = $1",
            vec![tx_hash.clone().into()],
        )
        .await?;
        assert_eq!(
            nonce, expectations.transaction_nonces[tx_hash],
            "tx nonce mismatch for hash {tx_hash}"
        );
    }

    for (receipt_id, expected_shard_id) in &expectations.receipt_shards {
        let shard_id = scalar_i64(
            db.connection.as_ref(),
            "SELECT shard_id::bigint AS value FROM near.near_receipts WHERE receipt_id = $1",
            vec![receipt_id.clone().into()],
        )
        .await?;
        assert_eq!(shard_id, *expected_shard_id, "receipt id {receipt_id}");

        let predecessor_id = scalar_string(
            db.connection.as_ref(),
            "SELECT predecessor_id::text AS value FROM near.near_receipts WHERE receipt_id = $1",
            vec![receipt_id.clone().into()],
        )
        .await?;
        assert_eq!(
            predecessor_id, expectations.receipt_predecessors[receipt_id],
            "receipt predecessor mismatch for receipt {receipt_id}"
        );

        let receiver_id = scalar_string(
            db.connection.as_ref(),
            "SELECT receiver_id::text AS value FROM near.near_receipts WHERE receipt_id = $1",
            vec![receipt_id.clone().into()],
        )
        .await?;
        assert_eq!(
            receiver_id, expectations.receipt_receivers[receipt_id],
            "receipt receiver mismatch for receipt {receipt_id}"
        );
    }

    for (outcome_id, expected_shard_id) in &expectations.outcome_shards {
        let shard_id = scalar_i64(
            db.connection.as_ref(),
            "SELECT shard_id::bigint AS value FROM near.near_execution_outcomes WHERE id = $1",
            vec![outcome_id.clone().into()],
        )
        .await?;
        assert_eq!(shard_id, *expected_shard_id, "outcome id {outcome_id}");

        let receipt_id = scalar_string(
            db.connection.as_ref(),
            "SELECT COALESCE(receipt_id, '') AS value FROM near.near_execution_outcomes WHERE id = $1",
            vec![outcome_id.clone().into()],
        )
        .await?;
        let expected_receipt = expectations.outcome_receipt_ids[outcome_id]
            .clone()
            .unwrap_or_default();
        assert_eq!(
            receipt_id, expected_receipt,
            "execution_outcome receipt_id mismatch for outcome {outcome_id}"
        );
    }

    for receipt_id in &expectations.local_receipt_ids {
        let row_count = scalar_i64(
            db.connection.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_receipts
             WHERE receipt_id = $1 AND block_height = $2",
            vec![receipt_id.clone().into(), height.into()],
        )
        .await?;
        assert_eq!(row_count, 1, "local receipt id {receipt_id}");
    }

    Ok(())
}

#[tokio::test]
async fn raw_module_is_replay_idempotent() -> Result<()> {
    init_test_tracing();

    let fixture = load_fixture("delegate_all");
    let height = fixture.block.header.height as i64;
    let first_tx_hash = fixture
        .shards
        .iter()
        .find_map(|shard| {
            shard
                .chunk
                .as_ref()
                .and_then(|chunk| chunk.transactions.first())
                .map(|item| item.transaction.hash.to_string())
        })
        .expect("fixture must include at least one transaction");

    let db = TestDatabase::new().await;
    let module = RawModule::new(&build_test_config());

    module.initialize(db.connection.as_ref()).await?;
    module
        .process_block(&fixture, db.connection.as_ref())
        .await?;

    let blocks_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_blocks WHERE height = $1",
        vec![height.into()],
    )
    .await?;
    let chunks_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_chunks WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    let tx_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_transactions WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    let receipts_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipts WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    let outcomes_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_execution_outcomes WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    let state_changes_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_state_changes WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    let tx_fingerprint_before = scalar_string(
        db.connection.as_ref(),
        "SELECT signer_id::text || '|' || receiver_id::text || '|' || nonce::text || '|' || shard_id::text AS value
         FROM near.near_transactions
         WHERE hash = $1",
        vec![first_tx_hash.clone().into()],
    )
    .await?;

    module
        .process_block(&fixture, db.connection.as_ref())
        .await?;

    let blocks_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_blocks WHERE height = $1",
        vec![height.into()],
    )
    .await?;
    let chunks_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_chunks WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    let tx_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_transactions WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    let receipts_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipts WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    let outcomes_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_execution_outcomes WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    let state_changes_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_state_changes WHERE block_height = $1",
        vec![height.into()],
    )
    .await?;
    let tx_fingerprint_after = scalar_string(
        db.connection.as_ref(),
        "SELECT signer_id::text || '|' || receiver_id::text || '|' || nonce::text || '|' || shard_id::text AS value
         FROM near.near_transactions
         WHERE hash = $1",
        vec![first_tx_hash.into()],
    )
    .await?;

    assert_eq!(blocks_before, blocks_after);
    assert_eq!(chunks_before, chunks_after);
    assert_eq!(tx_before, tx_after);
    assert_eq!(receipts_before, receipts_after);
    assert_eq!(outcomes_before, outcomes_after);
    assert_eq!(state_changes_before, state_changes_after);
    assert_eq!(tx_fingerprint_before, tx_fingerprint_after);

    Ok(())
}
