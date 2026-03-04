//! Integration test for veNEAR governance raw projection into canonical NEAR raw tables.

mod support;

use anyhow::Result;
use common::config::{
    AppConfig, DatabaseConfig, IndexerConfig, LoggingConfig, NearLakeConfig, NearModuleStreamMode,
    NearModulesConfig, NearStreamProvider, TelemetryConfig, VenearGovernanceModuleConfig,
};
use near_indexer::modules::venear_governance::VenearGovernanceModule;
use near_indexer::IndexerModule;
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::path::Path;
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
    "hos_approved_proposals",
    "hos_delegation_events",
    "hos_user_activities",
    "hos_proposals",
    "hos_registered_voters",
    "hos_config_history",
    "hos_proposal_non_voters",
];

fn load_fixture_range(name: &str) -> Vec<StreamerMessage> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/venear_governance/ranges")
        .join(format!("{name}.json"));

    let bytes = std::fs::read(&path)
        .unwrap_or_else(|err| panic!("Failed to read fixture {}: {err}", path.display()));

    serde_json::from_slice(&bytes)
        .unwrap_or_else(|err| panic!("Failed to deserialize fixture {}: {err}", path.display()))
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
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    for fixture in &fixtures {
        module.process_block(fixture, conn.as_ref()).await?;
    }

    let tracked_relevant_receipts = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.venear_governance_relevant_receipts",
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

    assert_eq!(
        blocks_written, 0,
        "venear_governance should not write near_blocks rows"
    );
    assert_eq!(
        chunks_written, 0,
        "venear_governance should not write near_chunks rows"
    );
    assert_eq!(
        transactions_written, 0,
        "venear_governance should not write near_transactions rows"
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
             FROM near.venear_governance_relevant_receipts rr
             WHERE rr.receipt_id = eo.receipt_id
         )",
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
             FROM near.venear_governance_relevant_receipts rr
             WHERE rr.receipt_id = ra.receipt_id
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
             FROM near.venear_governance_relevant_receipts rr_parent
             WHERE rr_parent.receipt_id = re.parent_receipt_id
         )
           AND EXISTS (
             SELECT 1
             FROM near.venear_governance_relevant_receipts rr_child
             WHERE rr_child.receipt_id = re.child_receipt_id
         )",
    )
    .await?;
    assert_eq!(
        edges_total, edges_between_relevant_receipts,
        "venear_governance should only write edges between tracked relevant receipts"
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
             'hos_approved_proposals',
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

    // Explicitly verify the injected non-HoS rows are excluded from HoS MVs.
    for sql in [
        "SELECT COUNT(*)::bigint AS value FROM near.hos_function_calls WHERE action_id = 'nonhos-action'",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_event_logs WHERE execution_outcome_id = 'nonhos-outcome'",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_proposal_voting_history WHERE receipt_id = 'nonhos-receipt'",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_approved_proposals WHERE receipt_id = 'nonhos-receipt'",
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
        "SELECT COUNT(*)::bigint AS value FROM near.hos_approved_proposals WHERE near.is_hos_contract(hos_contract_address) = FALSE",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_delegation_events WHERE near.is_hos_contract(hos_contract_address) = FALSE",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_proposals WHERE near.is_hos_contract(hos_contract_address) = FALSE",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_registered_voters WHERE near.is_hos_contract(hos_contract_address) = FALSE",
        "SELECT COUNT(*)::bigint AS value FROM near.hos_config_history WHERE near.is_hos_contract(contract_id) = FALSE",
    ] {
        let count = scalar_i64(conn.as_ref(), sql).await?;
        assert_eq!(count, 0, "HoS MV contract/action filter broken: {sql}");
    }

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
