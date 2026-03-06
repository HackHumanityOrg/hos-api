//! Integration tests for the raw NEAR module using ranged fixtures.

mod support;

use anyhow::Result;
use common::config::{
    AppConfig, DatabaseConfig, IndexerConfig, LoggingConfig, NearLakeConfig, NearModuleStreamMode,
    NearModulesConfig, NearStreamProvider, RawModuleConfig, TelemetryConfig,
};
use near_indexer::modules::raw::RawModule;
use near_indexer::IndexerModule;
use near_lake_framework::near_indexer_primitives::{
    views::{ReceiptEnumView, ReceiptView},
    StreamerMessage,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use support::hos_mv_goldens::{assert_hos_mv_golden_rows, assert_hos_mv_normalized_semantics};
use support::{init_test_tracing, TestDatabase};

const RANGE_SCENARIOS: &[&str] = &[
    "hos_lockup_core_167957",
    "hos_create_proposal_167957",
    "hos_approval_snapshot_168368",
    "hos_vote_168369",
    "hos_delegation_187483",
    "hos_config_185721",
];

#[derive(Debug, Default)]
struct RawFixtureExpectations {
    block_heights: HashSet<i64>,
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
    state_change_keys: HashSet<(i64, i64)>,
    receipt_action_ids: HashSet<String>,
    receipt_edge_keys: HashSet<(String, String)>,
}

fn load_fixture_range(name: &str) -> Vec<StreamerMessage> {
    let ranges_dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/venear_governance/ranges");
    let single_path = ranges_dir.join(format!("{name}.json"));

    let mut paths = Vec::new();
    if single_path.exists() {
        paths.push(single_path);
    } else {
        let part_prefix = format!("{name}.part");
        let mut part_paths: Vec<_> = std::fs::read_dir(&ranges_dir)
            .unwrap_or_else(|err| {
                panic!(
                    "Failed to read fixture directory {}: {err}",
                    ranges_dir.display()
                )
            })
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(|name| name.starts_with(&part_prefix) && name.ends_with(".json"))
                    .unwrap_or(false)
            })
            .collect();

        part_paths.sort();
        assert!(
            !part_paths.is_empty(),
            "Failed to find fixture range file or parts for {name} in {}",
            ranges_dir.display()
        );
        paths.extend(part_paths);
    }

    let mut messages = Vec::new();
    for path in paths {
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|err| panic!("Failed to read fixture {}: {err}", path.display()));
        let mut parsed: Vec<StreamerMessage> =
            serde_json::from_slice(&bytes).unwrap_or_else(|err| {
                panic!("Failed to deserialize fixture {}: {err}", path.display())
            });
        messages.append(&mut parsed);
    }

    messages
}

fn load_all_fixture_messages() -> Vec<StreamerMessage> {
    let mut messages = Vec::new();

    for scenario in RANGE_SCENARIOS {
        let mut range = load_fixture_range(scenario);
        assert!(
            !range.is_empty(),
            "fixture range should not be empty: {scenario}"
        );
        messages.append(&mut range);
    }

    messages.sort_by_key(|msg| msg.block.header.height);
    messages
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
                venear_governance: None,
            }),
        }),
    };

    AppConfig::Indexer(Box::new(cfg))
}

fn collect_expectations(messages: &[StreamerMessage]) -> RawFixtureExpectations {
    let mut expectations = RawFixtureExpectations::default();
    let mut persisted_data_receipt_parent_ids: HashMap<String, String> = HashMap::new();

    for msg in messages {
        let mut available_data_receipt_parent_ids = persisted_data_receipt_parent_ids.clone();
        let mut block_data_receipt_parent_ids = HashMap::new();
        seed_block_data_receipt_parent_ids(
            &mut available_data_receipt_parent_ids,
            &mut block_data_receipt_parent_ids,
            msg,
        );

        let block_height = msg.block.header.height as i64;
        expectations.block_heights.insert(block_height);

        for shard in &msg.shards {
            let shard_id = Into::<u64>::into(shard.shard_id) as i64;

            if !shard.state_changes.is_empty() {
                expectations
                    .state_change_keys
                    .insert((block_height, shard_id));
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

                        for child in &tx_with_outcome
                            .outcome
                            .execution_outcome
                            .outcome
                            .receipt_ids
                        {
                            expectations
                                .receipt_edge_keys
                                .insert((receipt.receipt_id.to_string(), child.to_string()));
                        }
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
                    note_data_receipt_edge(
                        &mut expectations.receipt_edge_keys,
                        &available_data_receipt_parent_ids,
                        receipt,
                    );
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
                    note_data_receipt_edge(
                        &mut expectations.receipt_edge_keys,
                        &available_data_receipt_parent_ids,
                        receipt,
                    );
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
                note_data_receipt_edge(
                    &mut expectations.receipt_edge_keys,
                    &available_data_receipt_parent_ids,
                    &outcome_with_receipt.receipt,
                );
                let receipt_id = outcome_with_receipt.receipt.receipt_id.to_string();

                expectations
                    .receipt_shards
                    .entry(receipt_id.clone())
                    .or_insert(shard_id);
                expectations
                    .receipt_predecessors
                    .entry(receipt_id.clone())
                    .or_insert_with(|| outcome_with_receipt.receipt.predecessor_id.to_string());
                expectations
                    .receipt_receivers
                    .entry(receipt_id.clone())
                    .or_insert_with(|| outcome_with_receipt.receipt.receiver_id.to_string());
                expectations
                    .outcome_shards
                    .entry(outcome_with_receipt.execution_outcome.id.to_string())
                    .or_insert(shard_id);
                expectations
                    .outcome_receipt_ids
                    .entry(outcome_with_receipt.execution_outcome.id.to_string())
                    .or_insert_with(|| Some(receipt_id.clone()));

                for child in &outcome_with_receipt.execution_outcome.outcome.receipt_ids {
                    expectations
                        .receipt_edge_keys
                        .insert((receipt_id.clone(), child.to_string()));
                }
                if let ReceiptEnumView::Action { actions, .. } =
                    &outcome_with_receipt.receipt.receipt
                {
                    for (action_index, _) in actions.iter().enumerate() {
                        expectations
                            .receipt_action_ids
                            .insert(format!("{receipt_id}-{action_index}"));
                    }
                }
            }
        }

        for (data_id, parent_receipt_id) in block_data_receipt_parent_ids {
            persisted_data_receipt_parent_ids.insert(data_id, parent_receipt_id);
        }
    }

    expectations
}

fn seed_block_data_receipt_parent_ids(
    available_data_receipt_parent_ids: &mut HashMap<String, String>,
    block_data_receipt_parent_ids: &mut HashMap<String, String>,
    message: &StreamerMessage,
) {
    for shard in &message.shards {
        if let Some(chunk) = &shard.chunk {
            for tx_with_outcome in &chunk.transactions {
                let Some(receipt) = &tx_with_outcome.outcome.receipt else {
                    continue;
                };
                note_output_data_receipt_parent_ids(available_data_receipt_parent_ids, receipt);
                note_output_data_receipt_parent_ids(block_data_receipt_parent_ids, receipt);
            }
        }

        for outcome_with_receipt in &shard.receipt_execution_outcomes {
            note_output_data_receipt_parent_ids(
                available_data_receipt_parent_ids,
                &outcome_with_receipt.receipt,
            );
            note_output_data_receipt_parent_ids(
                block_data_receipt_parent_ids,
                &outcome_with_receipt.receipt,
            );
        }
    }
}

fn note_data_receipt_edge(
    receipt_edge_keys: &mut HashSet<(String, String)>,
    available_data_receipt_parent_ids: &HashMap<String, String>,
    receipt: &ReceiptView,
) {
    let ReceiptEnumView::Data { data_id, .. } = &receipt.receipt else {
        return;
    };

    let Some(parent_receipt_id) = available_data_receipt_parent_ids.get(&data_id.to_string())
    else {
        return;
    };

    receipt_edge_keys.insert((parent_receipt_id.clone(), receipt.receipt_id.to_string()));
}

fn note_output_data_receipt_parent_ids(
    data_receipt_parent_ids_by_data_id: &mut HashMap<String, String>,
    receipt: &ReceiptView,
) {
    let ReceiptEnumView::Action {
        output_data_receivers,
        ..
    } = &receipt.receipt
    else {
        return;
    };

    for data_receiver in output_data_receivers {
        data_receipt_parent_ids_by_data_id
            .entry(data_receiver.data_id.to_string())
            .or_insert_with(|| receipt.receipt_id.to_string());
    }
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

async fn execute_sql(db: &impl ConnectionTrait, sql: &str) -> Result<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        vec![],
    ))
    .await?;
    Ok(())
}

#[tokio::test]
async fn raw_module_stores_complete_payloads_and_shard_ids() -> Result<()> {
    init_test_tracing();

    let fixtures = load_all_fixture_messages();
    let fixture_transactions_with_null_outcome_receipt: i64 = fixtures
        .iter()
        .flat_map(|fixture| fixture.shards.iter())
        .filter_map(|shard| shard.chunk.as_ref())
        .flat_map(|chunk| chunk.transactions.iter())
        .filter(|tx_with_outcome| tx_with_outcome.outcome.receipt.is_none())
        .count() as i64;
    let fixture_transactions_with_receipt_ids: i64 = fixtures
        .iter()
        .flat_map(|fixture| fixture.shards.iter())
        .filter_map(|shard| shard.chunk.as_ref())
        .flat_map(|chunk| chunk.transactions.iter())
        .filter(|tx_with_outcome| {
            !tx_with_outcome
                .outcome
                .execution_outcome
                .outcome
                .receipt_ids
                .is_empty()
        })
        .count() as i64;
    assert!(
        fixture_transactions_with_null_outcome_receipt > 0,
        "fixture regression: expected transactions where tx_with_outcome.outcome.receipt is null"
    );
    assert!(
        fixture_transactions_with_receipt_ids > 0,
        "fixture regression: expected transaction outcomes with non-empty receipt_ids"
    );

    let expectations = collect_expectations(&fixtures);
    assert!(
        !expectations.local_receipt_ids.is_empty(),
        "fixture suite should contain local receipts"
    );

    let db = TestDatabase::new().await;
    let module = RawModule::new(&build_test_config());

    module.initialize(db.connection.as_ref()).await?;
    for fixture in &fixtures {
        module
            .process_block(fixture, db.connection.as_ref())
            .await?;
    }

    let block_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_blocks",
        vec![],
    )
    .await?;
    assert_eq!(block_count, expectations.block_heights.len() as i64);

    let chunk_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_chunks",
        vec![],
    )
    .await?;
    assert_eq!(chunk_count, expectations.chunk_hashes.len() as i64);

    let tx_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_transactions",
        vec![],
    )
    .await?;
    assert_eq!(tx_count, expectations.transaction_shards.len() as i64);

    let receipt_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipts",
        vec![],
    )
    .await?;
    assert_eq!(receipt_count, expectations.receipt_shards.len() as i64);

    let outcome_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_execution_outcomes",
        vec![],
    )
    .await?;
    assert_eq!(outcome_count, expectations.outcome_shards.len() as i64);

    let state_change_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_state_changes",
        vec![],
    )
    .await?;
    assert_eq!(
        state_change_count,
        expectations.state_change_keys.len() as i64
    );

    let action_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_actions",
        vec![],
    )
    .await?;
    assert_eq!(action_count, expectations.receipt_action_ids.len() as i64);

    let edge_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_edges",
        vec![],
    )
    .await?;
    assert_eq!(edge_count, expectations.receipt_edge_keys.len() as i64);

    let mapped_receipts_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_tx_hash_map",
        vec![],
    )
    .await?;
    assert!(
        mapped_receipts_count > 0,
        "receipt->tx hash map should not be empty after fixture replay"
    );

    let receipts_missing_tx_map = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipts r
         WHERE EXISTS (
             SELECT 1
             FROM near.near_execution_outcomes eo
             WHERE eo.receipt_id = r.receipt_id
               AND eo.transaction_hash IS NOT NULL
         )
           AND NOT EXISTS (
             SELECT 1
             FROM near.near_receipt_tx_hash_map map
             WHERE map.receipt_id = r.receipt_id
         )",
        vec![],
    )
    .await?;
    assert_eq!(
        receipts_missing_tx_map, 0,
        "each executed receipt with known tx hash should have a lineage-map row"
    );

    let outcomes_without_tx_map = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_execution_outcomes eo
         WHERE eo.receipt_id IS NOT NULL
           AND eo.transaction_hash IS NOT NULL
           AND NOT EXISTS (
             SELECT 1
             FROM near.near_receipt_tx_hash_map map
             WHERE map.receipt_id = eo.receipt_id
               AND map.transaction_hash = eo.transaction_hash
         )",
        vec![],
    )
    .await?;
    assert_eq!(
        outcomes_without_tx_map, 0,
        "each execution outcome receipt with tx hash should be represented in lineage map"
    );

    let block_raw_nulls = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_blocks WHERE raw_data IS NULL",
        vec![],
    )
    .await?;
    assert_eq!(block_raw_nulls, 0);

    let chunk_raw_nulls = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_chunks WHERE raw_data IS NULL",
        vec![],
    )
    .await?;
    assert_eq!(chunk_raw_nulls, 0);

    let tx_raw_nulls = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_transactions WHERE raw_data IS NULL",
        vec![],
    )
    .await?;
    assert_eq!(tx_raw_nulls, 0);

    let receipt_raw_nulls = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipts WHERE raw_data IS NULL",
        vec![],
    )
    .await?;
    assert_eq!(receipt_raw_nulls, 0);

    let outcome_raw_nulls = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_execution_outcomes WHERE raw_data IS NULL",
        vec![],
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

    for action_id in &expectations.receipt_action_ids {
        let row_count = scalar_i64(
            db.connection.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_actions WHERE id = $1",
            vec![action_id.clone().into()],
        )
        .await?;
        assert_eq!(row_count, 1, "missing receipt action id {action_id}");
    }

    for (parent_receipt_id, child_receipt_id) in &expectations.receipt_edge_keys {
        let row_count = scalar_i64(
            db.connection.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_receipt_edges
             WHERE parent_receipt_id = $1 AND child_receipt_id = $2",
            vec![
                parent_receipt_id.clone().into(),
                child_receipt_id.clone().into(),
            ],
        )
        .await?;
        assert_eq!(
            row_count, 1,
            "missing receipt edge ({parent_receipt_id}, {child_receipt_id})"
        );
    }

    for receipt_id in &expectations.local_receipt_ids {
        let row_count = scalar_i64(
            db.connection.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_receipts WHERE receipt_id = $1",
            vec![receipt_id.clone().into()],
        )
        .await?;
        assert_eq!(row_count, 1, "local receipt id {receipt_id}");
    }

    Ok(())
}

#[tokio::test]
async fn raw_module_materialized_views_match_hos_golden_rows() -> Result<()> {
    init_test_tracing();

    let fixtures = load_all_fixture_messages();
    let db = TestDatabase::new().await;
    let module = RawModule::new(&build_test_config());

    module.initialize(db.connection.as_ref()).await?;
    for fixture in &fixtures {
        module
            .process_block(fixture, db.connection.as_ref())
            .await?;
    }

    execute_sql(
        db.connection.as_ref(),
        "SELECT near.refresh_hos_materialized_views()",
    )
    .await?;

    assert_hos_mv_golden_rows(db.connection.as_ref()).await?;
    assert_hos_mv_normalized_semantics(db.connection.as_ref()).await?;

    Ok(())
}

#[tokio::test]
async fn raw_module_is_replay_idempotent() -> Result<()> {
    init_test_tracing();

    let fixtures = load_all_fixture_messages();
    let first_tx_hash = fixtures
        .iter()
        .find_map(|fixture| {
            fixture.shards.iter().find_map(|shard| {
                shard
                    .chunk
                    .as_ref()
                    .and_then(|chunk| chunk.transactions.first())
                    .map(|item| item.transaction.hash.to_string())
            })
        })
        .expect("fixture suite must include at least one transaction");

    let db = TestDatabase::new().await;
    let module = RawModule::new(&build_test_config());

    module.initialize(db.connection.as_ref()).await?;
    for fixture in &fixtures {
        module
            .process_block(fixture, db.connection.as_ref())
            .await?;
    }

    let blocks_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_blocks",
        vec![],
    )
    .await?;
    let chunks_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_chunks",
        vec![],
    )
    .await?;
    let tx_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_transactions",
        vec![],
    )
    .await?;
    let receipts_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipts",
        vec![],
    )
    .await?;
    let outcomes_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_execution_outcomes",
        vec![],
    )
    .await?;
    let state_changes_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_state_changes",
        vec![],
    )
    .await?;
    let actions_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_actions",
        vec![],
    )
    .await?;
    let edges_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_edges",
        vec![],
    )
    .await?;
    let tx_map_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_tx_hash_map",
        vec![],
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

    for fixture in &fixtures {
        module
            .process_block(fixture, db.connection.as_ref())
            .await?;
    }

    let blocks_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_blocks",
        vec![],
    )
    .await?;
    let chunks_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_chunks",
        vec![],
    )
    .await?;
    let tx_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_transactions",
        vec![],
    )
    .await?;
    let receipts_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipts",
        vec![],
    )
    .await?;
    let outcomes_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_execution_outcomes",
        vec![],
    )
    .await?;
    let state_changes_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_state_changes",
        vec![],
    )
    .await?;
    let actions_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_actions",
        vec![],
    )
    .await?;
    let edges_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_edges",
        vec![],
    )
    .await?;
    let tx_map_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_tx_hash_map",
        vec![],
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
    assert_eq!(actions_before, actions_after);
    assert_eq!(edges_before, edges_after);
    assert_eq!(tx_map_before, tx_map_after);
    assert_eq!(tx_fingerprint_before, tx_fingerprint_after);

    Ok(())
}
