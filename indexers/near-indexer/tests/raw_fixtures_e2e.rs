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
use support::fixture_catalog::{
    all_fixture_cases, load_all_range_fixture_messages, load_fixture_messages, range_fixture_cases,
    ModuleExpectationKind,
};
use support::fixture_expectations::{
    assert_fixture_expectation, assert_snapshot_equal, capture_fixture_snapshot,
    refresh_hos_materialized_views, reset_near_indexer_tables,
};
use support::hos_mv_goldens::assert_hos_mv_normalized_semantics;
use support::{init_test_tracing, skip_resource_heavy_test_in_ci, TestDatabase};

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

#[derive(Debug)]
struct CrossBlockDataReceiptCase {
    parent_fixture_index: usize,
    child_fixture_index: usize,
    parent_receipt_id: String,
    child_receipt_id: String,
}

fn load_all_fixture_messages() -> Vec<StreamerMessage> {
    load_all_range_fixture_messages().expect("failed to load ranged fixture messages")
}

fn find_cross_block_data_receipt_fixture_case(
) -> Result<(String, Vec<StreamerMessage>, CrossBlockDataReceiptCase)> {
    for fixture_case in range_fixture_cases()? {
        let messages = load_fixture_messages(&fixture_case)?;
        if let Some(case) = first_cross_block_data_receipt_case(&messages) {
            return Ok((fixture_case.id, messages, case));
        }
    }

    anyhow::bail!("fixture suite must include a cross-block data receipt")
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
                    ..RawModuleConfig::default()
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

fn first_cross_block_data_receipt_case(
    messages: &[StreamerMessage],
) -> Option<CrossBlockDataReceiptCase> {
    let mut parent_links = HashMap::new();

    for (fixture_index, message) in messages.iter().enumerate() {
        for shard in &message.shards {
            if let Some(chunk) = &shard.chunk {
                for tx_with_outcome in &chunk.transactions {
                    let Some(receipt) = &tx_with_outcome.outcome.receipt else {
                        continue;
                    };
                    remember_data_receipt_parent_link(&mut parent_links, fixture_index, receipt);
                }

                for receipt in chunk.receipts.iter().chain(chunk.local_receipts.iter()) {
                    if let Some(case) =
                        resolve_cross_block_data_receipt_case(&parent_links, fixture_index, receipt)
                    {
                        return Some(case);
                    }
                }
            }

            for outcome_with_receipt in &shard.receipt_execution_outcomes {
                remember_data_receipt_parent_link(
                    &mut parent_links,
                    fixture_index,
                    &outcome_with_receipt.receipt,
                );
                if let Some(case) = resolve_cross_block_data_receipt_case(
                    &parent_links,
                    fixture_index,
                    &outcome_with_receipt.receipt,
                ) {
                    return Some(case);
                }
            }
        }
    }

    None
}

fn remember_data_receipt_parent_link(
    parent_links: &mut HashMap<String, (usize, String)>,
    fixture_index: usize,
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
        parent_links
            .entry(data_receiver.data_id.to_string())
            .or_insert_with(|| (fixture_index, receipt.receipt_id.to_string()));
    }
}

fn resolve_cross_block_data_receipt_case(
    parent_links: &HashMap<String, (usize, String)>,
    child_fixture_index: usize,
    receipt: &ReceiptView,
) -> Option<CrossBlockDataReceiptCase> {
    let ReceiptEnumView::Data { data_id, .. } = &receipt.receipt else {
        return None;
    };

    let (parent_fixture_index, parent_receipt_id) = parent_links.get(&data_id.to_string())?;
    if *parent_fixture_index >= child_fixture_index {
        return None;
    }

    Some(CrossBlockDataReceiptCase {
        parent_fixture_index: *parent_fixture_index,
        child_fixture_index,
        parent_receipt_id: parent_receipt_id.clone(),
        child_receipt_id: receipt.receipt_id.to_string(),
    })
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

#[tokio::test]
async fn raw_module_stores_complete_payloads_and_shard_ids() -> Result<()> {
    init_test_tracing();
    if skip_resource_heavy_test_in_ci("raw_module_stores_complete_payloads_and_shard_ids") {
        return Ok(());
    }

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
async fn raw_module_fixture_expectations_match_checked_in_snapshots() -> Result<()> {
    init_test_tracing();
    if skip_resource_heavy_test_in_ci("raw_module_fixture_expectations_match_checked_in_snapshots")
    {
        return Ok(());
    }

    let db = TestDatabase::new().await;

    for fixture_case in all_fixture_cases()? {
        reset_near_indexer_tables(db.connection.as_ref()).await?;

        let module = RawModule::new(&build_test_config());
        let fixtures = load_fixture_messages(&fixture_case)?;
        module.initialize(db.connection.as_ref()).await?;
        for fixture in &fixtures {
            module
                .process_block(fixture, db.connection.as_ref())
                .await?;
        }

        refresh_hos_materialized_views(db.connection.as_ref()).await?;
        assert_fixture_expectation(
            db.connection.as_ref(),
            &fixture_case,
            ModuleExpectationKind::Raw,
        )
        .await?;
        assert_hos_mv_normalized_semantics(db.connection.as_ref()).await?;
    }

    Ok(())
}

#[tokio::test]
async fn raw_module_is_replay_idempotent() -> Result<()> {
    init_test_tracing();
    if skip_resource_heavy_test_in_ci("raw_module_is_replay_idempotent") {
        return Ok(());
    }

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
    refresh_hos_materialized_views(db.connection.as_ref()).await?;
    let snapshot_before_replay = capture_fixture_snapshot(db.connection.as_ref()).await?;

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
    refresh_hos_materialized_views(db.connection.as_ref()).await?;
    let snapshot_after_replay = capture_fixture_snapshot(db.connection.as_ref()).await?;

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
    assert_snapshot_equal(
        &snapshot_after_replay,
        &snapshot_before_replay,
        "raw-module-replay-idempotence",
    )?;

    Ok(())
}

#[tokio::test]
async fn raw_module_rebuilds_data_receipt_lineage_after_replay_restart() -> Result<()> {
    init_test_tracing();
    if skip_resource_heavy_test_in_ci(
        "raw_module_rebuilds_data_receipt_lineage_after_replay_restart",
    ) {
        return Ok(());
    }

    let (fixture_id, fixtures, case) = find_cross_block_data_receipt_fixture_case()?;

    let baseline_db = TestDatabase::new().await;
    let baseline_module = RawModule::new(&build_test_config());
    baseline_module
        .initialize(baseline_db.connection.as_ref())
        .await?;
    for fixture in &fixtures[..=case.child_fixture_index] {
        baseline_module
            .process_block(fixture, baseline_db.connection.as_ref())
            .await?;
    }

    let expected_edge_count = scalar_i64(
        baseline_db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_edges
         WHERE parent_receipt_id = $1
           AND child_receipt_id = $2",
        vec![
            case.parent_receipt_id.clone().into(),
            case.child_receipt_id.clone().into(),
        ],
    )
    .await?;
    assert_eq!(
        expected_edge_count, 1,
        "fixture {fixture_id} must resolve the baseline receipt edge"
    );

    let expected_edge_fingerprint = scalar_string(
        baseline_db.connection.as_ref(),
        "SELECT parent_execution_outcome_id::text
                || '|' || block_height::text
                || '|' || block_hash::text
                || '|' || block_timestamp::text
                || '|' || COALESCE(transaction_hash::text, '') AS value
         FROM near.near_receipt_edges
         WHERE parent_receipt_id = $1
           AND child_receipt_id = $2",
        vec![
            case.parent_receipt_id.clone().into(),
            case.child_receipt_id.clone().into(),
        ],
    )
    .await?;

    let replay_db = TestDatabase::new().await;
    let first_run_module = RawModule::new(&build_test_config());
    first_run_module
        .initialize(replay_db.connection.as_ref())
        .await?;
    for fixture in &fixtures[..=case.parent_fixture_index] {
        first_run_module
            .process_block(fixture, replay_db.connection.as_ref())
            .await?;
    }

    let restarted_module = RawModule::new(&build_test_config());
    restarted_module
        .initialize(replay_db.connection.as_ref())
        .await?;
    for fixture in &fixtures[case.parent_fixture_index..=case.child_fixture_index] {
        restarted_module
            .process_block(fixture, replay_db.connection.as_ref())
            .await?;
    }

    let edge_count = scalar_i64(
        replay_db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_edges
         WHERE parent_receipt_id = $1
           AND child_receipt_id = $2",
        vec![
            case.parent_receipt_id.clone().into(),
            case.child_receipt_id.clone().into(),
        ],
    )
    .await?;
    assert_eq!(edge_count, 1);

    let edge_fingerprint = scalar_string(
        replay_db.connection.as_ref(),
        "SELECT parent_execution_outcome_id::text
                || '|' || block_height::text
                || '|' || block_hash::text
                || '|' || block_timestamp::text
                || '|' || COALESCE(transaction_hash::text, '') AS value
         FROM near.near_receipt_edges
         WHERE parent_receipt_id = $1
           AND child_receipt_id = $2",
        vec![case.parent_receipt_id.into(), case.child_receipt_id.into()],
    )
    .await?;
    assert_eq!(edge_fingerprint, expected_edge_fingerprint);

    Ok(())
}

#[tokio::test]
async fn raw_module_persists_exact_shared_protocol_fields() -> Result<()> {
    init_test_tracing();
    if skip_resource_heavy_test_in_ci("raw_module_persists_exact_shared_protocol_fields") {
        return Ok(());
    }

    let fixtures = load_all_fixture_messages();
    let db = TestDatabase::new().await;
    let module = RawModule::new(&build_test_config());

    module.initialize(db.connection.as_ref()).await?;
    for fixture in &fixtures {
        module
            .process_block(fixture, db.connection.as_ref())
            .await?;
    }

    let mismatched_tokens = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_execution_outcomes
         WHERE COALESCE(outcome ->> 'tokens_burnt', raw_data #>> '{outcome,tokens_burnt}', '0')
               <> tokens_burnt_yocto",
        vec![],
    )
    .await?;
    assert_eq!(mismatched_tokens, 0);

    let mismatched_blocks = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_blocks
         WHERE hash <> raw_data #>> '{header,hash}'
            OR prev_hash <> raw_data #>> '{header,prev_hash}'
            OR epoch_id <> raw_data #>> '{header,epoch_id}'
            OR next_epoch_id <> raw_data #>> '{header,next_epoch_id}'
            OR author <> raw_data ->> 'author'
            OR timestamp::text <> raw_data #>> '{header,timestamp}'
            OR timestamp_nanosec::text <> raw_data #>> '{header,timestamp_nanosec}'
            OR gas_price <> raw_data #>> '{header,gas_price}'
            OR total_supply <> raw_data #>> '{header,total_supply}'
            OR validator_reward <> raw_data #>> '{header,validator_reward}'
            OR protocol_version::text <> raw_data #>> '{header,latest_protocol_version}'
            OR chunks_included::text <> raw_data #>> '{header,chunks_included}'
            OR COALESCE(block_ordinal::text, '') <> COALESCE(raw_data #>> '{header,block_ordinal}', '')
            OR COALESCE(prev_height::text, '') <> COALESCE(raw_data #>> '{header,prev_height}', '')",
        vec![],
    )
    .await?;
    assert_eq!(mismatched_blocks, 0);

    let mismatched_chunks = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_chunks
         WHERE chunk_hash <> raw_data #>> '{header,chunk_hash}'
            OR author <> raw_data ->> 'author'
            OR height_created::text <> raw_data #>> '{header,height_created}'
            OR height_included::text <> raw_data #>> '{header,height_included}'
            OR gas_used::text <> raw_data #>> '{header,gas_used}'
            OR gas_limit::text <> raw_data #>> '{header,gas_limit}'
            OR validator_reward <> raw_data #>> '{header,validator_reward}'",
        vec![],
    )
    .await?;
    assert_eq!(mismatched_chunks, 0);

    let mismatched_transactions = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_transactions
         WHERE hash <> raw_data ->> 'hash'
            OR signer_id <> raw_data ->> 'signer_id'
            OR receiver_id <> raw_data ->> 'receiver_id'
            OR nonce::text <> raw_data ->> 'nonce'
            OR priority_fee::text <> raw_data ->> 'priority_fee'
            OR public_key <> raw_data ->> 'public_key'
            OR signature <> raw_data ->> 'signature'
            OR actions <> raw_data -> 'actions'",
        vec![],
    )
    .await?;
    assert_eq!(mismatched_transactions, 0);

    let mismatched_receipts = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipts
         WHERE receipt_id <> raw_data ->> 'receipt_id'
            OR predecessor_id <> raw_data ->> 'predecessor_id'
            OR receiver_id <> raw_data ->> 'receiver_id'
            OR receipt <> raw_data -> 'receipt'",
        vec![],
    )
    .await?;
    assert_eq!(mismatched_receipts, 0);

    let mismatched_execution_outcomes = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_execution_outcomes
         WHERE id <> raw_data ->> 'id'
            OR outcome <> raw_data -> 'outcome'
            OR block_hash <> raw_data ->> 'block_hash'
            OR gas_burnt::text <> raw_data #>> '{outcome,gas_burnt}'
            OR tokens_burnt_yocto <> raw_data #>> '{outcome,tokens_burnt}'
            OR executor_account_id <> raw_data #>> '{outcome,executor_id}'
            OR outcome_receipt_ids <> raw_data #> '{outcome,receipt_ids}'
            OR logs <> raw_data #> '{outcome,logs}'
            OR executed_in_block_hash <> raw_data ->> 'block_hash'",
        vec![],
    )
    .await?;
    assert_eq!(mismatched_execution_outcomes, 0);

    let mismatched_receipt_actions = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_actions ra
         JOIN near.near_receipts r
           ON r.receipt_id = ra.receipt_id
         WHERE action_json <> (r.receipt -> 'Action' -> 'actions' -> ra.action_index)
            OR action_kind IS DISTINCT FROM CASE jsonb_typeof(action_json)
                WHEN 'object' THEN (
                    SELECT key
                    FROM jsonb_each(action_json)
                    LIMIT 1
                )
                WHEN 'string' THEN trim(both '\"' from action_json::text)
                ELSE 'Unknown'
            END
            OR signer_account_id IS DISTINCT FROM (r.receipt -> 'Action' ->> 'signer_id')
            OR signer_public_key IS DISTINCT FROM (r.receipt -> 'Action' ->> 'signer_public_key')
            OR gas_price IS DISTINCT FROM (r.receipt -> 'Action' ->> 'gas_price')
            OR method_name IS DISTINCT FROM (action_json -> 'FunctionCall' ->> 'method_name')
            OR COALESCE(gas::text, '') <> COALESCE(action_json -> 'FunctionCall' ->> 'gas', '')
            OR COALESCE(deposit, '') <> COALESCE(action_json -> 'FunctionCall' ->> 'deposit', '')
            OR COALESCE(args_base64, '') <> COALESCE(action_json -> 'FunctionCall' ->> 'args', '')",
        vec![],
    )
    .await?;
    assert_eq!(mismatched_receipt_actions, 0);

    Ok(())
}
