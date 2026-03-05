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

fn build_venear_only_config() -> AppConfig {
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
                raw: None,
                venear_governance: Some(VenearGovernanceModuleConfig {
                    enabled: true,
                    stream_mode: NearModuleStreamMode::Shared,
                    start_height: None,
                    mview_refresh_interval_blocks: 0,
                }),
            }),
        }),
    };

    AppConfig::Indexer(Box::new(cfg))
}

fn build_raw_and_venear_config() -> AppConfig {
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
                venear_governance: Some(VenearGovernanceModuleConfig {
                    enabled: true,
                    stream_mode: NearModuleStreamMode::Shared,
                    start_height: None,
                    mview_refresh_interval_blocks: 0,
                }),
            }),
        }),
    };

    AppConfig::Indexer(Box::new(cfg))
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
async fn venear_governance_projects_to_canonical_raw_tables_and_hos_materialized_views(
) -> Result<()> {
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

    for fixture in &fixtures {
        module.process_block(fixture, conn.as_ref()).await?;
    }

    let tracked_relevant_receipts = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_relevant_receipts
         WHERE scope = 'hos'",
    )
    .await?;
    assert!(
        tracked_relevant_receipts > 0,
        "expected tracked HoS relevant receipts"
    );

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
             FROM near.near_relevant_receipts rr
             WHERE rr.scope = 'hos'
               AND rr.discovered_at_block_height = b.height
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
    let transactions_written = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_transactions",
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
    assert_eq!(
        chunks_written, 0,
        "venear_governance should not write near_chunks rows"
    );
    assert!(
        transactions_written > 0,
        "venear_governance should write HoS-related near_transactions rows"
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
    let outcomes_on_relevant_receipts = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_execution_outcomes eo
         WHERE EXISTS (
             SELECT 1
             FROM near.near_relevant_receipts rr
             WHERE rr.scope = 'hos'
               AND rr.receipt_id = eo.receipt_id
         )",
    )
    .await?;
    let non_hos_scope_rows = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_relevant_receipts
         WHERE scope <> 'hos'",
    )
    .await?;
    assert!(
        outcomes_total > 0,
        "expected canonical near_execution_outcomes rows"
    );
    assert_eq!(
        outcomes_total, outcomes_on_relevant_receipts,
        "venear_governance should only write outcomes for tracked relevant receipts"
    );
    assert_eq!(
        non_hos_scope_rows, 0,
        "venear-only run should not create non-hos relevance rows"
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
    let actions_on_relevant_receipts = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_actions ra
         WHERE EXISTS (
             SELECT 1
             FROM near.near_relevant_receipts rr
             WHERE rr.scope = 'hos'
               AND rr.receipt_id = ra.receipt_id
         )",
    )
    .await?;
    assert!(
        actions_total > 0,
        "expected canonical near_receipt_actions rows"
    );
    assert_eq!(
        actions_total, actions_on_relevant_receipts,
        "venear_governance should only write actions for tracked relevant receipts"
    );

    let edges_total = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_receipt_edges",
    )
    .await?;
    let edges_between_relevant_receipts = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_edges re
         WHERE EXISTS (
             SELECT 1
             FROM near.near_relevant_receipts rr_parent
             WHERE rr_parent.scope = 'hos'
               AND rr_parent.receipt_id = re.parent_receipt_id
         )
           AND EXISTS (
             SELECT 1
             FROM near.near_relevant_receipts rr_child
             WHERE rr_child.scope = 'hos'
               AND rr_child.receipt_id = re.child_receipt_id
         )",
    )
    .await?;
    assert_eq!(
        edges_total, edges_between_relevant_receipts,
        "venear_governance should only write edges between tracked relevant receipts"
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
    let relevant_with_tx_hash = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_relevant_receipts
         WHERE scope = 'hos' AND transaction_hash IS NOT NULL",
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
        relevant_with_tx_hash > 0,
        "HoS relevant_receipts should include non-null transaction_hash values"
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
           UNION
           SELECT DISTINCT transaction_hash AS hash
           FROM near.near_relevant_receipts
           WHERE scope = 'hos' AND transaction_hash IS NOT NULL
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
    assert!(
        placeholder_transactions > 0,
        "fixture regression: expected at least one HoS fallback near_transactions placeholder row"
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
async fn raw_and_venear_modules_can_share_relevant_receipts_by_scope() -> Result<()> {
    init_test_tracing();

    let fixtures = load_all_fixture_messages();
    let db = TestDatabase::new().await;
    let conn = db.db();
    let config = build_raw_and_venear_config();
    let raw_module = RawModule::new(&config);
    let venear_module = VenearGovernanceModule::new(&config);

    raw_module.initialize(conn.as_ref()).await?;
    venear_module.initialize(conn.as_ref()).await?;

    for fixture in &fixtures {
        raw_module.process_block(fixture, conn.as_ref()).await?;
        venear_module.process_block(fixture, conn.as_ref()).await?;
    }

    let all_scope_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_relevant_receipts
         WHERE scope = 'all'",
    )
    .await?;
    let hos_scope_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_relevant_receipts
         WHERE scope = 'hos'",
    )
    .await?;
    let overlap_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_relevant_receipts all_scope
         INNER JOIN near.near_relevant_receipts hos_scope
           ON hos_scope.receipt_id = all_scope.receipt_id
         WHERE all_scope.scope = 'all'
           AND hos_scope.scope = 'hos'",
    )
    .await?;
    let hos_missing_from_all = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_relevant_receipts hos_scope
         WHERE hos_scope.scope = 'hos'
           AND NOT EXISTS (
             SELECT 1
             FROM near.near_relevant_receipts all_scope
             WHERE all_scope.scope = 'all'
               AND all_scope.receipt_id = hos_scope.receipt_id
           )",
    )
    .await?;

    assert!(
        all_scope_count > 0,
        "expected relevant receipts for scope=all"
    );
    assert!(
        hos_scope_count > 0,
        "expected relevant receipts for scope=hos"
    );
    assert!(
        overlap_count > 0,
        "expected at least one receipt tracked in both scopes"
    );
    assert_eq!(
        hos_missing_from_all, 0,
        "all-scope rows should cover hos-scope rows when raw is enabled"
    );

    Ok(())
}
