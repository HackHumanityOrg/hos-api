//! Integration test for veNEAR governance raw projection into canonical NEAR raw tables.

mod support;

use anyhow::Result;
use common::config::{
    AppConfig, DatabaseConfig, IndexerConfig, LoggingConfig, NearLakeConfig, NearModuleStreamMode,
    NearModulesConfig, NearStreamProvider, RawModuleConfig, TelemetryConfig,
    VenearGovernanceModuleConfig,
};
use near_indexer::modules::raw::RawModule;
use near_indexer::modules::venear_governance::VenearGovernanceModule;
use near_indexer::IndexerModule;
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::path::Path;
use std::sync::OnceLock;
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

const HOS_MATERIALIZED_VIEWS: &[&str] = &[
    "hos_function_calls",
    "hos_event_logs",
    "hos_proposal_voting_history",
    "hos_delegation_events",
    "hos_user_activities",
    "hos_proposals",
    "hos_registered_voters",
    "hos_config_history",
    "hos_proposal_non_voters",
];

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

fn all_fixture_messages() -> &'static [StreamerMessage] {
    static FIXTURES: OnceLock<Vec<StreamerMessage>> = OnceLock::new();
    FIXTURES.get_or_init(load_all_fixture_messages).as_slice()
}

fn core_hos_fixture_messages() -> &'static [StreamerMessage] {
    static FIXTURES: OnceLock<Vec<StreamerMessage>> = OnceLock::new();
    FIXTURES
        .get_or_init(|| {
            let mut messages = load_fixture_range("hos_lockup_core_167957");
            messages.sort_by_key(|msg| msg.block.header.height);
            messages
        })
        .as_slice()
}

fn rewards_fixture_messages() -> &'static [StreamerMessage] {
    static FIXTURES: OnceLock<Vec<StreamerMessage>> = OnceLock::new();
    FIXTURES
        .get_or_init(|| {
            let mut messages = load_fixture_range("hos_rewards_claim_179510");
            messages.sort_by_key(|msg| msg.block.header.height);
            messages
        })
        .as_slice()
}

fn base_fixture_message() -> StreamerMessage {
    static BASE_FIXTURE: OnceLock<StreamerMessage> = OnceLock::new();
    BASE_FIXTURE
        .get_or_init(|| {
            all_fixture_messages()
                .first()
                .cloned()
                .expect("fixture suite must include at least one block")
        })
        .clone()
}

fn build_test_app_config(
    raw: Option<RawModuleConfig>,
    venear_governance: Option<VenearGovernanceModuleConfig>,
) -> AppConfig {
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
                raw,
                venear_governance,
            }),
        }),
    };

    AppConfig::Indexer(Box::new(cfg))
}

fn raw_config(enabled: bool) -> RawModuleConfig {
    RawModuleConfig {
        enabled,
        stream_mode: NearModuleStreamMode::Shared,
        start_height: None,
    }
}

fn venear_test_config() -> VenearGovernanceModuleConfig {
    VenearGovernanceModuleConfig {
        enabled: true,
        stream_mode: NearModuleStreamMode::Shared,
        start_height: None,
        mview_refresh_interval_blocks: 0,
        finalization_interval_blocks: 1,
        restart_rewind_blocks: 256,
        ..VenearGovernanceModuleConfig::default()
    }
}

fn build_venear_only_config() -> AppConfig {
    build_test_app_config(Some(raw_config(false)), Some(venear_test_config()))
}

fn build_default_raw_and_venear_config() -> AppConfig {
    build_test_app_config(None, Some(venear_test_config()))
}

fn build_raw_and_venear_config() -> AppConfig {
    build_test_app_config(Some(raw_config(true)), Some(venear_test_config()))
}

fn effective_receipt_tx_map_retention_blocks(cfg: &VenearGovernanceModuleConfig) -> u64 {
    cfg.receipt_tx_map_retention_blocks.max(
        cfg.restart_rewind_blocks
            .saturating_add(200)
            .saturating_add(cfg.finalization_interval_blocks),
    )
}

fn is_hos_test_account(account_id: &str) -> bool {
    matches!(
        account_id,
        "venear.dao" | "vote.dao" | "hos-root.sputnik-dao.near" | "rewards.hos-dao.near"
    ) || account_id
        .strip_suffix(".venear.dao")
        .map(|prefix| {
            prefix.len() == 40
                && prefix
                    .as_bytes()
                    .iter()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        })
        .unwrap_or(false)
}

fn first_root_hos_descendant_fixture(fixtures: &[StreamerMessage]) -> Option<(u64, String)> {
    fixtures.iter().find_map(|msg| {
        msg.shards
            .iter()
            .filter_map(|shard| shard.chunk.as_ref())
            .flat_map(|chunk| chunk.transactions.iter())
            .find(|tx_with_outcome| {
                !tx_with_outcome
                    .outcome
                    .execution_outcome
                    .outcome
                    .receipt_ids
                    .is_empty()
                    && (is_hos_test_account(tx_with_outcome.transaction.receiver_id.as_ref())
                        || is_hos_test_account(tx_with_outcome.transaction.signer_id.as_ref()))
            })
            .map(|tx_with_outcome| {
                (
                    msg.block.header.height,
                    tx_with_outcome.transaction.hash.to_string(),
                )
            })
    })
}

async fn scalar_i64(db: &impl ConnectionTrait, sql: &str) -> Result<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![],
        ))
        .await?
        .expect("expected one row from scalar query");
    Ok(row.try_get("", "value")?)
}

async fn scalar_bool_param(db: &impl ConnectionTrait, sql: &str, value: &str) -> Result<bool> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![value.into()],
        ))
        .await?
        .expect("expected one row from scalar bool query");
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

async fn first_string(db: &impl ConnectionTrait, sql: &str) -> Result<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![],
        ))
        .await?
        .expect("expected one row from scalar string query");
    Ok(row.try_get("", "value")?)
}

fn cleanup_trigger_message(block_height: u64) -> StreamerMessage {
    let mut msg = base_fixture_message();
    msg.block.header.height = block_height;
    msg.block.header.prev_height = Some(block_height.saturating_sub(1));
    msg.shards.clear();
    msg
}

fn live_fixture_message() -> StreamerMessage {
    base_fixture_message()
}

async fn insert_receipt_tx_map_row(
    db: &impl ConnectionTrait,
    receipt_id: &str,
    transaction_hash: &str,
    first_seen_height: i64,
    last_seen_height: i64,
) -> Result<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO near.near_receipt_tx_hash_map (
            receipt_id,
            transaction_hash,
            first_seen_height,
            last_seen_height,
            created_at,
            updated_at
        ) VALUES ($1, $2, $3, $4, NOW(), NOW())",
        vec![
            receipt_id.into(),
            transaction_hash.into(),
            first_seen_height.into(),
            last_seen_height.into(),
        ],
    ))
    .await?;
    Ok(())
}

async fn insert_canonical_transaction_row(
    db: &impl ConnectionTrait,
    transaction_hash: &str,
    block_height: i64,
) -> Result<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO near.near_transactions (
            hash,
            block_height,
            signer_id,
            receiver_id,
            nonce,
            priority_fee,
            public_key,
            signature,
            actions,
            raw_data,
            shard_id
        ) VALUES (
            $1,
            $2,
            'signer.test.near',
            'receiver.test.near',
            1,
            0,
            'ed25519:test',
            'signature',
            '[]'::jsonb,
            '{}'::jsonb,
            0
        )",
        vec![transaction_hash.into(), block_height.into()],
    ))
    .await?;
    Ok(())
}

#[tokio::test]
async fn venear_governance_projects_to_canonical_raw_tables_and_hos_materialized_views(
) -> Result<()> {
    init_test_tracing();

    let fixtures = all_fixture_messages();
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

    let fixture_receipt_outcomes: i64 = fixtures
        .iter()
        .map(|fixture| {
            fixture
                .shards
                .iter()
                .map(|shard| shard.receipt_execution_outcomes.len() as i64)
                .sum::<i64>()
        })
        .sum();

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_venear_only_config());
    module.initialize(conn.as_ref()).await?;

    for fixture in fixtures {
        module.process_block(fixture, conn.as_ref()).await?;
    }

    let blocks_written = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_blocks",
    )
    .await?;
    let blocks_without_hos_relevance = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_blocks b
         WHERE NOT EXISTS (
             SELECT 1
             FROM near.near_execution_outcomes eo
             WHERE eo.block_height = b.height
         )
           AND NOT EXISTS (
             SELECT 1
             FROM near.near_transactions nt
             WHERE nt.block_height = b.height
         )",
    )
    .await?;
    let distinct_outcome_blocks = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(DISTINCT block_height)::bigint AS value
         FROM near.near_execution_outcomes",
    )
    .await?;
    let chunks_written = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_chunks",
    )
    .await?;
    let state_changes_written = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_state_changes",
    )
    .await?;
    let transactions_written = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_transactions",
    )
    .await?;
    let hos_action_transactions = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(DISTINCT transaction_hash)::bigint AS value
         FROM near.near_receipt_actions
         WHERE transaction_hash IS NOT NULL
           AND near.is_hos_action(receiver_id, predecessor_id, signer_account_id)",
    )
    .await?;

    assert!(
        blocks_written > 0,
        "venear_governance should write HoS-related near_blocks rows"
    );
    assert_eq!(
        blocks_without_hos_relevance, 0,
        "venear_governance should not write non-HoS near_blocks rows"
    );
    assert!(
        blocks_written >= distinct_outcome_blocks,
        "near_blocks should cover all HoS execution outcome blocks"
    );
    assert!(
        chunks_written > 0,
        "venear_governance should write HoS-related near_chunks rows"
    );
    assert!(
        state_changes_written > 0,
        "venear_governance should write HoS-related near_state_changes rows"
    );
    assert!(
        transactions_written > 0,
        "venear_governance should write HoS-related near_transactions rows"
    );
    assert!(
        hos_action_transactions > 0,
        "venear_governance should produce canonical HoS-matching actions"
    );

    let receipts_total = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipts",
    )
    .await?;
    assert!(receipts_total > 0, "expected canonical near_receipts rows");

    let outcomes_total = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_execution_outcomes",
    )
    .await?;
    assert!(
        outcomes_total > 0,
        "expected canonical near_execution_outcomes rows"
    );
    assert!(
        outcomes_total < fixture_receipt_outcomes,
        "venear_governance should filter the fixture stream to HoS-relevant outcomes"
    );

    let actions_total = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_actions",
    )
    .await?;
    assert!(
        actions_total > 0,
        "expected canonical near_receipt_actions rows"
    );

    let edges_total = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_edges",
    )
    .await?;
    assert!(
        edges_total > 0,
        "expected canonical near_receipt_edges rows"
    );

    let outcomes_with_tx_hash = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_execution_outcomes
         WHERE transaction_hash IS NOT NULL",
    )
    .await?;
    let actions_with_tx_hash = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_actions
         WHERE transaction_hash IS NOT NULL",
    )
    .await?;
    let edges_with_tx_hash = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_edges
         WHERE transaction_hash IS NOT NULL",
    )
    .await?;
    let mapped_hos_receipts = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_tx_hash_map map
         WHERE EXISTS (
             SELECT 1
             FROM near.near_receipt_actions ra
             WHERE ra.transaction_hash = map.transaction_hash
               AND near.is_hos_action(
                     ra.receiver_id,
                     ra.predecessor_id,
                     ra.signer_account_id
               )
         )",
    )
    .await?;
    assert!(
        outcomes_with_tx_hash > 0,
        "HoS near_execution_outcomes should include non-null transaction_hash values"
    );
    assert!(
        actions_with_tx_hash > 0,
        "HoS near_receipt_actions should include non-null transaction_hash values"
    );
    assert!(
        edges_with_tx_hash > 0,
        "HoS near_receipt_edges should include non-null transaction_hash values"
    );
    assert!(
        mapped_hos_receipts > 0,
        "HoS lineage map should include non-null transaction_hash values"
    );
    let orphaned_hos_transaction_hashes = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM (
           SELECT DISTINCT transaction_hash AS hash
           FROM near.near_execution_outcomes
           WHERE transaction_hash IS NOT NULL
           UNION
           SELECT DISTINCT transaction_hash AS hash
           FROM near.near_receipt_actions
           WHERE transaction_hash IS NOT NULL
         UNION
         SELECT DISTINCT transaction_hash AS hash
         FROM near.near_receipt_edges
         WHERE transaction_hash IS NOT NULL
         ) canonical_hashes
         WHERE NOT EXISTS (
           SELECT 1
           FROM near.near_transactions nt
           WHERE nt.hash = canonical_hashes.hash
         )",
    )
    .await?;
    assert_eq!(
        orphaned_hos_transaction_hashes, 0,
        "all HoS canonical transaction_hash values must resolve to near_transactions rows"
    );

    let placeholder_transactions = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_transactions
         WHERE raw_data IS NULL",
    )
    .await?;
    assert_eq!(
        placeholder_transactions, 0,
        "venear_governance should not use placeholder near_transactions rows anymore"
    );

    let legacy_hos_raw_views = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM information_schema.views
         WHERE table_schema = 'near'
           AND table_name IN ('hos_receipt_actions', 'hos_execution_outcomes')",
    )
    .await?;
    assert_eq!(
        legacy_hos_raw_views, 0,
        "raw compatibility views should not exist anymore"
    );

    let hos_config_table_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM information_schema.tables
         WHERE table_schema = 'near'
           AND table_name = 'hos_config'",
    )
    .await?;
    assert_eq!(
        hos_config_table_count, 0,
        "hos_config should not exist as a table"
    );

    // Inject one explicit non-HoS raw action/outcome pair. HoS MVs must still
    // exclude it via their own filters.
    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_execution_outcomes (
            id,
            block_height,
            receipt_id,
            outcome,
            raw_data,
            shard_id,
            transaction_hash,
            block_hash,
            chunk_hash,
            gas_burnt,
            gas_used,
            tokens_burnt,
            executor_account_id,
            status,
            outcome_receipt_ids,
            executed_in_block_hash,
            logs,
            results_json,
            block_timestamp
         ) VALUES (
            'nonhos-outcome',
            999000001,
            'nonhos-receipt',
            '{}'::jsonb,
            NULL,
            0,
            'nonhos-tx',
            'nonhos-bh',
            'nonhos-ch',
            0,
            0,
            0,
            'example.near',
            'SuccessValue',
            '[]'::jsonb,
            'nonhos-bh',
            jsonb_build_array('EVENT_JSON:{\"event\":\"ft_mint\",\"data\":[{\"amount\":\"1\"}]}'),
            NULL,
            999000001000000000
         )
         ON CONFLICT (id) DO NOTHING",
    )
    .await?;

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_receipt_actions (
            id,
            block_height,
            receipt_id,
            execution_outcome_id,
            transaction_hash,
            shard_id,
            signer_account_id,
            signer_public_key,
            gas_price,
            action_kind,
            action_json,
            predecessor_id,
            receiver_id,
            block_hash,
            chunk_hash,
            author,
            method_name,
            gas,
            deposit,
            args_base64,
            args_json,
            action_index,
            block_timestamp
         ) VALUES (
            'nonhos-action',
            999000001,
            'nonhos-receipt',
            'nonhos-outcome',
            'nonhos-tx',
            0,
            'alice.near',
            'ed25519:nonhos',
            '0',
            'FunctionCall',
            '{\"FunctionCall\":{\"method_name\":\"vote\"}}'::jsonb,
            'alice.near',
            'example.near',
            'nonhos-bh',
            'nonhos-ch',
            'author.near',
            'vote',
            0,
            '0',
            NULL,
            '{\"proposal_id\":\"999\",\"vote\":\"1\"}'::jsonb,
            0,
            999000001000000000
         )
         ON CONFLICT (id) DO NOTHING",
    )
    .await?;

    execute_sql(
        conn.as_ref(),
        "SELECT near.refresh_hos_materialized_views()",
    )
    .await?;

    // Validate that all HoS MVs exist and can be queried.
    let materialized_view_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM pg_matviews
         WHERE schemaname = 'near'
           AND matviewname IN (
             'hos_function_calls',
             'hos_event_logs',
             'hos_proposal_voting_history',
             'hos_delegation_events',
             'hos_user_activities',
             'hos_proposals',
             'hos_registered_voters',
             'hos_config_history',
             'hos_proposal_non_voters'
           )",
    )
    .await?;
    assert_eq!(
        materialized_view_count,
        HOS_MATERIALIZED_VIEWS.len() as i64,
        "expected all HoS materialized views"
    );

    for view in HOS_MATERIALIZED_VIEWS {
        let count = scalar_i64(
            conn.as_ref(),
            &format!("SELECT COUNT(*)::bigint AS value FROM near.{view}"),
        )
        .await?;

        if *view != "hos_proposal_non_voters" {
            assert!(count > 0, "expected {view} to be non-empty");
        }
    }

    let function_calls_with_tx_hash = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.hos_function_calls
         WHERE transaction_hash IS NOT NULL",
    )
    .await?;
    let event_logs_with_tx_hash = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.hos_event_logs
         WHERE transaction_hash IS NOT NULL",
    )
    .await?;
    let voting_history_with_tx_hash = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.hos_proposal_voting_history
         WHERE transaction_hash IS NOT NULL",
    )
    .await?;
    let delegation_with_tx_hash = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.hos_delegation_events
         WHERE transaction_hash IS NOT NULL",
    )
    .await?;
    let activities_with_tx_hash = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.hos_user_activities
         WHERE transaction_hash IS NOT NULL",
    )
    .await?;
    let approved_proposals_with_approval_tx_hash = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.hos_proposals
         WHERE is_approved = TRUE
           AND approval_transaction_hash IS NOT NULL",
    )
    .await?;
    let approved_proposals_total = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.hos_proposals
         WHERE is_approved = TRUE",
    )
    .await?;
    assert!(
        function_calls_with_tx_hash > 0,
        "hos_function_calls should include non-null transaction_hash values"
    );
    assert!(
        event_logs_with_tx_hash > 0,
        "hos_event_logs should include non-null transaction_hash values"
    );
    assert!(
        voting_history_with_tx_hash > 0,
        "hos_proposal_voting_history should include non-null transaction_hash values"
    );
    assert!(
        delegation_with_tx_hash > 0,
        "hos_delegation_events should include non-null transaction_hash values"
    );
    assert!(
        activities_with_tx_hash > 0,
        "hos_user_activities should include non-null transaction_hash values"
    );
    assert!(
        approved_proposals_total == 0 || approved_proposals_with_approval_tx_hash > 0,
        "approved hos_proposals rows should include non-null approval_transaction_hash values"
    );

    // Explicitly verify the injected non-HoS rows are excluded from HoS MVs.
    for sql in [
        "SELECT COUNT(*)::bigint AS value FROM near.hos_function_calls WHERE action_id = 'nonhos-action'",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_event_logs WHERE execution_outcome_id = 'nonhos-outcome'",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_proposal_voting_history WHERE receipt_id = 'nonhos-receipt'",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_delegation_events WHERE receipt_id = 'nonhos-receipt'",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_user_activities WHERE receipt_id = 'nonhos-receipt'",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_proposals WHERE receipt_id = 'nonhos-receipt'",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_registered_voters WHERE receipt_id = 'nonhos-receipt'",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_config_history WHERE receipt_id = 'nonhos-receipt'",
    ] {
        let count = scalar_i64(conn.as_ref(), sql).await?;
        assert_eq!(count, 0, "non-HoS row leaked into a HoS MV: {sql}");
    }

    // HoS filtering invariants on each materialized view.
    for sql in [
        "SELECT COUNT(*)::bigint AS value FROM near.hos_function_calls WHERE near.is_hos_action(receiver_id, predecessor_id, signer_account_id) = FALSE",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_event_logs WHERE near.is_hos_contract(contract_id) = FALSE",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_proposal_voting_history WHERE near.is_hos_contract(hos_contract_address) = FALSE",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_delegation_events WHERE near.is_hos_contract(hos_contract_address) = FALSE",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_proposals WHERE near.is_hos_contract(hos_contract_address) = FALSE",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_registered_voters WHERE near.is_hos_contract(hos_contract_address) = FALSE",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_config_history WHERE near.is_hos_contract(contract_id) = FALSE",
    ] {
        let count = scalar_i64(conn.as_ref(), sql).await?;
        assert_eq!(count, 0, "HoS MV contract/action filter broken: {sql}");
    }

    assert_hos_mv_golden_rows(conn.as_ref()).await?;
    assert_hos_mv_normalized_semantics(conn.as_ref()).await?;

    let non_voter_overlap = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.hos_proposal_non_voters nv
         INNER JOIN near.hos_proposal_voting_history v
           ON v.proposal_id = nv.proposal_id
          AND v.voter_id = nv.registered_voter_id",
    )
    .await?;
    assert_eq!(
        non_voter_overlap, 0,
        "proposal_non_voters must not include accounts that actually voted"
    );

    Ok(())
}

#[tokio::test]
async fn omitted_raw_config_still_prunes_stale_receipt_tx_map_rows() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();
    let mut venear_cfg = venear_test_config();
    venear_cfg.receipt_tx_map_retention_blocks = 0;
    let mut config = build_default_raw_and_venear_config();
    if let AppConfig::Indexer(indexer) = &mut config {
        let near = indexer
            .near
            .as_mut()
            .expect("test config should include near settings");
        let modules = near
            .modules
            .as_mut()
            .expect("test config should include module settings");
        modules.venear_governance = Some(venear_cfg);
    }
    let raw_module = RawModule::new(&config);
    let venear_module = VenearGovernanceModule::new(&config);

    raw_module.initialize(conn.as_ref()).await?;
    venear_module.initialize(conn.as_ref()).await?;

    insert_receipt_tx_map_row(conn.as_ref(), "receipt-legacy", "tx-legacy", 0, 0).await?;

    let fixture = live_fixture_message();
    raw_module.process_block(&fixture, conn.as_ref()).await?;
    venear_module.process_block(&fixture, conn.as_ref()).await?;

    let canonical_transactions = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_transactions",
    )
    .await?;
    assert!(
        canonical_transactions > 0,
        "omitted raw config should still run the raw module"
    );

    let legacy_map_rows = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_tx_hash_map
         WHERE receipt_id = 'receipt-legacy'
           AND transaction_hash = 'tx-legacy'",
    )
    .await?;
    assert_eq!(
        legacy_map_rows, 0,
        "venear cleanup should prune stale lineage rows even when raw is implicitly enabled"
    );

    Ok(())
}

#[tokio::test]
async fn sql_hos_contract_matcher_matches_verified_accounts() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();

    for account_id in [
        "venear.dao",
        "vote.dao",
        "hos-root.sputnik-dao.near",
        "rewards.hos-dao.near",
        "0887fe221cafb6810909c83393e23585c439fe99.venear.dao",
    ] {
        assert!(
            scalar_bool_param(
                conn.as_ref(),
                "SELECT near.is_hos_contract($1) AS value",
                account_id,
            )
            .await?,
            "expected {account_id} to match near.is_hos_contract"
        );
    }

    for account_id in [
        "dao",
        "foo.vote.dao",
        "lockup.alice.venear.dao",
        "random.dao",
    ] {
        assert!(
            !scalar_bool_param(
                conn.as_ref(),
                "SELECT near.is_hos_contract($1) AS value",
                account_id,
            )
            .await?,
            "expected {account_id} to be excluded from near.is_hos_contract"
        );
    }

    Ok(())
}

#[tokio::test]
async fn venear_governance_includes_rewards_contract_activity() -> Result<()> {
    init_test_tracing();

    let fixtures = rewards_fixture_messages();
    let rewards_tx_hash = fixtures
        .iter()
        .flat_map(|fixture| fixture.shards.iter())
        .filter_map(|shard| shard.chunk.as_ref())
        .flat_map(|chunk| chunk.transactions.iter())
        .find(|tx_with_outcome| {
            tx_with_outcome.transaction.signer_id.as_str() == "rewards.hos-dao.near"
                || tx_with_outcome.transaction.receiver_id.as_str() == "rewards.hos-dao.near"
        })
        .map(|tx_with_outcome| tx_with_outcome.transaction.hash.to_string())
        .expect("expected rewards fixture to contain a rewards.hos-dao.near transaction");

    let db = TestDatabase::new().await;
    let conn = db.db();
    let mut venear_cfg = venear_test_config();
    venear_cfg.finalization_interval_blocks = 1;
    let module = VenearGovernanceModule::new(&build_test_app_config(
        Some(raw_config(false)),
        Some(venear_cfg),
    ));

    module.initialize(conn.as_ref()).await?;
    for fixture in fixtures {
        module.process_block(fixture, conn.as_ref()).await?;
    }

    let rewards_tx_rows = scalar_i64(
        conn.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_transactions
             WHERE hash = '{rewards_tx_hash}'"
        ),
    )
    .await?;
    assert_eq!(rewards_tx_rows, 1);

    let rewards_receipt_rows = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipts
         WHERE predecessor_id = 'rewards.hos-dao.near'
            OR receiver_id = 'rewards.hos-dao.near'",
    )
    .await?;
    assert!(
        rewards_receipt_rows >= 2,
        "expected rewards contract flow receipts to be promoted"
    );

    let rewards_to_lockup_receipts = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipts
         WHERE predecessor_id = 'rewards.hos-dao.near'
           AND receiver_id LIKE '%.venear.dao'",
    )
    .await?;
    assert!(
        rewards_to_lockup_receipts > 0,
        "expected rewards claim flow to include a lockup transfer child receipt"
    );

    let system_refund_receipts = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipts
         WHERE predecessor_id = 'system'
           AND receiver_id = 'hos-governance.near'",
    )
    .await?;
    assert!(
        system_refund_receipts > 0,
        "expected claim flow to retain the sibling system refund receipt"
    );

    let rewards_outcomes = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_execution_outcomes eo
         WHERE EXISTS (
             SELECT 1
             FROM near.near_receipts r
             WHERE r.receipt_id = eo.receipt_id
               AND (r.predecessor_id = 'rewards.hos-dao.near'
                    OR r.receiver_id = 'rewards.hos-dao.near')
         )",
    )
    .await?;
    assert!(
        rewards_outcomes > 0,
        "expected rewards contract execution outcomes to be promoted"
    );

    let rewards_chunks = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_chunks
         WHERE block_height BETWEEN 179510973 AND 179510975",
    )
    .await?;
    assert!(rewards_chunks > 0);

    let rewards_state_changes = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_state_changes
         WHERE block_height BETWEEN 179510973 AND 179510975",
    )
    .await?;
    assert!(rewards_state_changes > 0);

    Ok(())
}

#[tokio::test]
async fn stalled_pending_graphs_remain_unpromoted_until_descendants_arrive() -> Result<()> {
    init_test_tracing();

    let fixtures = core_hos_fixture_messages();
    let (initial_block_height, tx_hash) = first_root_hos_descendant_fixture(fixtures)
        .expect("expected a root HoS fixture transaction with descendants");
    let initial_block = fixtures
        .iter()
        .find(|msg| msg.block.header.height == initial_block_height)
        .cloned()
        .expect("expected to find initial descendant block");

    let db = TestDatabase::new().await;
    let conn = db.db();
    let mut venear_cfg = venear_test_config();
    venear_cfg.finalization_interval_blocks = 3;
    let module = VenearGovernanceModule::new(&build_test_app_config(
        Some(raw_config(false)),
        Some(venear_cfg),
    ));

    module.initialize(conn.as_ref()).await?;
    module.process_block(&initial_block, conn.as_ref()).await?;
    module
        .process_block(&cleanup_trigger_message(43_203), conn.as_ref())
        .await?;

    let promoted_before_descendants = scalar_i64(
        conn.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_transactions
             WHERE hash = '{tx_hash}'"
        ),
    )
    .await?;
    assert_eq!(
        promoted_before_descendants, 0,
        "an unresolved HoS graph should stay out of canonical tables"
    );

    for fixture in fixtures
        .iter()
        .filter(|fixture| fixture.block.header.height > initial_block_height)
    {
        module.process_block(fixture, conn.as_ref()).await?;
    }
    let last_height = fixtures
        .last()
        .expect("fixtures should not be empty")
        .block
        .header
        .height;
    let finalize_height = ((last_height / 3) + 1) * 3;
    module
        .process_block(&cleanup_trigger_message(finalize_height), conn.as_ref())
        .await?;

    let promoted_after_descendants = scalar_i64(
        conn.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_transactions
             WHERE hash = '{tx_hash}'"
        ),
    )
    .await?;
    assert_eq!(
        promoted_after_descendants, 1,
        "the graph should promote once its descendants arrive"
    );

    Ok(())
}

#[tokio::test]
async fn receipt_tx_map_cleanup_deletes_orphans_after_retention() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();
    let mut venear_cfg = venear_test_config();
    venear_cfg.finalization_interval_blocks = 1;
    venear_cfg.receipt_tx_map_retention_blocks = 1;
    let cleanup_height = effective_receipt_tx_map_retention_blocks(&venear_cfg) + 1;
    let module = VenearGovernanceModule::new(&build_test_app_config(
        Some(raw_config(false)),
        Some(venear_cfg),
    ));

    module.initialize(conn.as_ref()).await?;
    insert_receipt_tx_map_row(conn.as_ref(), "receipt-orphan", "tx-orphan", 0, 0).await?;

    module
        .process_block(&cleanup_trigger_message(cleanup_height), conn.as_ref())
        .await?;

    let map_rows_after_cleanup = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_tx_hash_map
         WHERE receipt_id = 'receipt-orphan'",
    )
    .await?;
    assert_eq!(
        map_rows_after_cleanup, 0,
        "cleanup should remove stale orphan lineage rows"
    );

    Ok(())
}

#[tokio::test]
async fn receipt_tx_map_cleanup_preserves_canonical_and_active_graph_rows() -> Result<()> {
    init_test_tracing();

    let fixtures = core_hos_fixture_messages();
    let (initial_block_height, active_tx_hash) = first_root_hos_descendant_fixture(fixtures)
        .expect("expected a root HoS fixture transaction with descendants");
    let initial_block = fixtures
        .iter()
        .find(|msg| msg.block.header.height == initial_block_height)
        .cloned()
        .expect("expected to find initial descendant block");

    let db = TestDatabase::new().await;
    let conn = db.db();
    let mut venear_cfg = venear_test_config();
    venear_cfg.finalization_interval_blocks = 1;
    venear_cfg.receipt_tx_map_retention_blocks = 1;
    let cleanup_height =
        initial_block_height + effective_receipt_tx_map_retention_blocks(&venear_cfg) + 1;
    let module = VenearGovernanceModule::new(&build_test_app_config(
        Some(raw_config(false)),
        Some(venear_cfg),
    ));

    module.initialize(conn.as_ref()).await?;
    module.process_block(&initial_block, conn.as_ref()).await?;

    let active_receipt_id = first_string(
        conn.as_ref(),
        &format!(
            "SELECT receipt_id AS value
             FROM near.near_receipt_tx_hash_map
             WHERE transaction_hash = '{active_tx_hash}'
             ORDER BY receipt_id
             LIMIT 1"
        ),
    )
    .await?;

    insert_receipt_tx_map_row(conn.as_ref(), "receipt-delete", "tx-delete", 0, 0).await?;
    insert_receipt_tx_map_row(conn.as_ref(), "receipt-canonical", "tx-canonical", 0, 0).await?;
    insert_canonical_transaction_row(conn.as_ref(), "tx-canonical", 0).await?;

    module
        .process_block(&cleanup_trigger_message(cleanup_height), conn.as_ref())
        .await?;

    let deleted_rows = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_tx_hash_map
         WHERE receipt_id = 'receipt-delete'",
    )
    .await?;
    assert_eq!(
        deleted_rows, 0,
        "stale orphaned lineage rows should be deleted"
    );

    let canonical_rows = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_tx_hash_map
         WHERE receipt_id = 'receipt-canonical'",
    )
    .await?;
    assert_eq!(
        canonical_rows, 0,
        "HoS-only cleanup should prune stale canonical lineage rows once they are no longer active"
    );

    let active_rows = scalar_i64(
        conn.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_receipt_tx_hash_map
             WHERE receipt_id = '{active_receipt_id}'
               AND transaction_hash = '{active_tx_hash}'"
        ),
    )
    .await?;
    assert_eq!(
        active_rows, 1,
        "active in-memory graphs must preserve lineage rows"
    );

    Ok(())
}

#[tokio::test]
async fn late_related_receipts_reopen_finalized_graphs() -> Result<()> {
    init_test_tracing();

    let fixtures = core_hos_fixture_messages();
    let (replay_block_height, tx_hash) = first_root_hos_descendant_fixture(fixtures)
        .expect("expected a root HoS fixture transaction with descendants");
    let replay_block = fixtures
        .iter()
        .find(|msg| msg.block.header.height == replay_block_height)
        .cloned()
        .expect("expected to find replay block");

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_venear_only_config());

    module.initialize(conn.as_ref()).await?;
    for fixture in fixtures {
        module.process_block(fixture, conn.as_ref()).await?;
    }

    let tx_count_before = scalar_i64(
        conn.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_transactions
             WHERE hash = '{tx_hash}'"
        ),
    )
    .await?;
    let outcome_count_before = scalar_i64(
        conn.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_execution_outcomes
             WHERE transaction_hash = '{tx_hash}'"
        ),
    )
    .await?;
    let edge_count_before = scalar_i64(
        conn.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_receipt_edges
             WHERE transaction_hash = '{tx_hash}'"
        ),
    )
    .await?;

    module.process_block(&replay_block, conn.as_ref()).await?;

    let tx_count_after = scalar_i64(
        conn.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_transactions
             WHERE hash = '{tx_hash}'"
        ),
    )
    .await?;
    let outcome_count_after = scalar_i64(
        conn.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_execution_outcomes
             WHERE transaction_hash = '{tx_hash}'"
        ),
    )
    .await?;
    let edge_count_after = scalar_i64(
        conn.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_receipt_edges
             WHERE transaction_hash = '{tx_hash}'"
        ),
    )
    .await?;

    assert_eq!(tx_count_after, tx_count_before);
    assert_eq!(outcome_count_after, outcome_count_before);
    assert_eq!(edge_count_after, edge_count_before);

    Ok(())
}

#[tokio::test]
async fn raw_and_venear_modules_share_receipt_tx_lineage_map() -> Result<()> {
    init_test_tracing();

    let fixtures = all_fixture_messages();
    let db = TestDatabase::new().await;
    let conn = db.db();
    let config = build_raw_and_venear_config();
    let raw_module = RawModule::new(&config);
    let venear_module = VenearGovernanceModule::new(&config);

    raw_module.initialize(conn.as_ref()).await?;
    venear_module.initialize(conn.as_ref()).await?;

    for fixture in fixtures {
        raw_module.process_block(fixture, conn.as_ref()).await?;
        venear_module.process_block(fixture, conn.as_ref()).await?;
    }

    let tx_map_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_tx_hash_map",
    )
    .await?;
    let mapped_hos_receipts = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_tx_hash_map map
         WHERE EXISTS (
             SELECT 1
             FROM near.near_receipt_actions ra
             WHERE ra.transaction_hash = map.transaction_hash
               AND near.is_hos_action(
                     ra.receiver_id,
                     ra.predecessor_id,
                     ra.signer_account_id
               )
         )",
    )
    .await?;
    let mapped_non_hos_receipts = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_tx_hash_map map
         WHERE NOT EXISTS (
             SELECT 1
             FROM near.near_receipt_actions ra
             WHERE ra.transaction_hash = map.transaction_hash
               AND near.is_hos_action(
                     ra.receiver_id,
                     ra.predecessor_id,
                     ra.signer_account_id
               )
         )",
    )
    .await?;
    assert!(tx_map_count > 0, "expected receipt->tx lineage rows");
    assert!(
        mapped_hos_receipts > 0,
        "expected lineage rows for HoS transactions"
    );
    assert!(
        mapped_non_hos_receipts > 0,
        "raw should contribute lineage rows beyond HoS-only scope"
    );

    Ok(())
}
