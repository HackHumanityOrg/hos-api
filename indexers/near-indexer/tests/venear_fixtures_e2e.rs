//! Integration tests for the veNEAR governance module using fixture files.
//!
//! Each test loads one or more pre-recorded `StreamerMessage` JSON files from
//! `tests/fixtures/venear_governance/`, processes them through
//! `VenearGovernanceModule` backed by a real Postgres instance (testcontainers),
//! and asserts correct DB state across ALL relevant tables.
//!
//! Fixtures are generated once by `examples/fetch_venear_fixtures.rs` and then
//! committed to the repository. Tests are intentionally real-fixture-first;
//! if a rare fixture is currently unavailable on-chain, the corresponding test
//! is skipped with a clear message instead of synthesizing fake chain data.

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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Load a fixture file by event-type name.
/// Returns `None` when the file does not exist (test will be skipped).
fn try_load_fixture(name: &str) -> Option<StreamerMessage> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/venear_governance")
        .join(format!("{name}.json"));

    if !path.exists() {
        return None;
    }

    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("Failed to read fixture {}: {e}", path.display()));

    Some(
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|e| panic!("Failed to deserialize fixture {}: {e}", path.display())),
    )
}

fn load_fixture(name: &str) -> StreamerMessage {
    try_load_fixture(name).unwrap_or_else(|| panic!("Required fixture not found: {name}.json"))
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
                accounts: None,
                tokens: None,
                staking_pools: None,
                venear_governance: Some(VenearGovernanceModuleConfig {
                    enabled: true,
                    stream_mode: NearModuleStreamMode::Shared,
                    start_height: None,
                    venear_contract_id: "venear.dao".to_string(),
                    voting_contract_id: "vote.dao".to_string(),
                    lockup_account_suffix: ".venear.dao".to_string(),
                    known_lockups: vec![],
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Lockup deployed → timeseries row in near_venear_lockup_events; raw event recorded.
/// Tables NOT affected: near_venear_proposal_events, near_venear_votes.
#[tokio::test]
async fn test_lockup_deployed() -> Result<()> {
    init_test_tracing();

    let fixture = load_fixture("lockup_deployed");

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&fixture, conn.as_ref()).await?;

    // near_venear_lockup_events: lockup_deployed row for a .venear.dao account
    let lockup_deployed = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE event_type = 'lockup_deployed'",
    )
    .await?;
    assert!(
        lockup_deployed >= 1,
        "expected >= 1 lockup_deployed lockup event row, got {lockup_deployed}"
    );

    let lockup_suffix = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE lockup_account_id LIKE '%.venear.dao'",
    )
    .await?;
    assert!(
        lockup_suffix >= 1,
        "expected lockup_account_id ending in .venear.dao, got {lockup_suffix}"
    );

    // near_venear_events_raw: lockup_deployed event recorded with venear standard
    let raw_deployed = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
         WHERE event_type = 'lockup_deployed' AND standard = 'venear'",
    )
    .await?;
    assert!(
        raw_deployed >= 1,
        "expected >= 1 raw event row with event_type='lockup_deployed', got {raw_deployed}"
    );

    // near_venear_lockup_events: lockup_deployed_callback from the on_lockup_deployed receipt.
    // The lockup_deployed event is always emitted from within the on_lockup_deployed callback,
    // so any block with a lockup_deployed event must also have the callback receipt.
    let callback_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE event_type = 'lockup_deployed_callback'",
    )
    .await?;
    assert!(
        callback_count >= 1,
        "expected >= 1 lockup_deployed_callback row in near_venear_lockup_events, got {callback_count}"
    );

    // near_venear_proposal_events: must be empty (this block has no proposals)
    let proposal_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events",
    )
    .await?;
    assert_eq!(
        proposal_count, 0,
        "expected 0 proposal events from lockup_deployed block, got {proposal_count}"
    );

    // near_venear_votes: must be empty
    let vote_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes",
    )
    .await?;
    assert_eq!(
        vote_count, 0,
        "expected 0 vote rows from lockup_deployed block, got {vote_count}"
    );

    Ok(())
}

/// Lockup update → entry in near_venear_account_balances with lockup fields.
/// The lockup_update fixture (block 167957476) also contains an on_lockup_update callback,
/// so near_venear_lockup_events is also expected to have a row.
#[tokio::test]
async fn test_lockup_update() -> Result<()> {
    init_test_tracing();

    let fixture = load_fixture("lockup_update");

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&fixture, conn.as_ref()).await?;

    // near_venear_account_balances: lockup_update event with lockup-specific fields
    let balance_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances
         WHERE event_type = 'lockup_update'",
    )
    .await?;
    assert!(
        balance_count >= 1,
        "expected >= 1 lockup_update balance row, got {balance_count}"
    );

    let balance_with_locked = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances
         WHERE event_type = 'lockup_update' AND locked_near_balance IS NOT NULL",
    )
    .await?;
    assert!(
        balance_with_locked >= 1,
        "expected lockup_update rows to have locked_near_balance set, got {balance_with_locked}"
    );

    let balance_with_nonce = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances
         WHERE event_type = 'lockup_update' AND lockup_update_nonce IS NOT NULL",
    )
    .await?;
    assert!(
        balance_with_nonce >= 1,
        "expected lockup_update rows to have lockup_update_nonce set, got {balance_with_nonce}"
    );

    // near_venear_lockup_events: lockup callback/event rows exist in the same block
    let lockup_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events",
    )
    .await?;
    assert!(
        lockup_count >= 1,
        "expected >= 1 lockup row (from on_lockup_update in same block), got {lockup_count}"
    );

    // near_venear_events_raw: lockup_update event recorded
    let raw_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
         WHERE event_type = 'lockup_update' AND standard = 'venear'",
    )
    .await?;
    assert!(
        raw_count >= 1,
        "expected >= 1 raw event row with event_type='lockup_update', got {raw_count}"
    );

    // near_venear_proposal_events: must be empty
    let proposal_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events",
    )
    .await?;
    assert_eq!(
        proposal_count, 0,
        "expected 0 proposal events from lockup_update block, got {proposal_count}"
    );

    // near_venear_votes: must be empty
    let vote_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes",
    )
    .await?;
    assert_eq!(
        vote_count, 0,
        "expected 0 vote rows from lockup_update block, got {vote_count}"
    );

    Ok(())
}

/// Lockup delete → lockup_delete row in near_venear_lockup_events.
#[tokio::test]
async fn test_lockup_delete() -> Result<()> {
    init_test_tracing();

    let deployed = load_fixture("lockup_deployed");
    let deleted = load_fixture("lockup_delete");

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&deployed, conn.as_ref()).await?;
    module.process_block(&deleted, conn.as_ref()).await?;

    // near_venear_lockup_events: at least one lockup_delete row
    let deleted_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE event_type = 'lockup_delete'",
    )
    .await?;
    assert!(
        deleted_count >= 1,
        "expected >= 1 lockup_delete event row after lockup_delete, got {deleted_count}"
    );

    // near_venear_events_raw: both lockup_deployed and lockup_delete events recorded
    let raw_delete = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
         WHERE event_type = 'lockup_delete'",
    )
    .await?;
    assert!(
        raw_delete >= 1,
        "expected >= 1 raw event row with event_type='lockup_delete', got {raw_delete}"
    );

    // near_venear_proposal_events: must be empty
    let proposal_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events",
    )
    .await?;
    assert_eq!(
        proposal_count, 0,
        "expected 0 proposal events from lockup_delete test, got {proposal_count}"
    );

    Ok(())
}

/// lockup_lock_near event from a lockup contract is captured as a lockup event.
#[tokio::test]
async fn test_lockup_lock_near() -> Result<()> {
    init_test_tracing();

    let Some(fixture) = try_load_fixture("lockup_lock_near") else {
        eprintln!("SKIP test_lockup_lock_near: lockup_lock_near fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&fixture, conn.as_ref()).await?;

    let lockup_event_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE event_type = 'lockup_lock_near'",
    )
    .await?;
    assert!(
        lockup_event_count >= 1,
        "expected >= 1 lockup_lock_near row, got {lockup_event_count}"
    );

    let lockup_with_balance = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE event_type = 'lockup_lock_near' AND locked_near_balance IS NOT NULL",
    )
    .await?;
    assert!(
        lockup_with_balance >= 1,
        "expected lockup_lock_near row with locked_near_balance set, got {lockup_with_balance}"
    );

    let raw_event_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
         WHERE event_type = 'lockup_lock_near' AND standard = 'venear'",
    )
    .await?;
    assert!(
        raw_event_count >= 1,
        "expected >= 1 raw lockup_lock_near event row, got {raw_event_count}"
    );

    Ok(())
}

/// Proposal lifecycle: create → approve → finalized callback (on_get_snapshot).
/// Tables NOT affected: near_venear_votes, near_venear_lockup_actions.
#[tokio::test]
async fn test_proposal_lifecycle() -> Result<()> {
    init_test_tracing();

    let Some(create) = try_load_fixture("create_proposal") else {
        eprintln!("SKIP test_proposal_lifecycle: create_proposal fixture not found");
        return Ok(());
    };
    let Some(approve) = try_load_fixture("proposal_approve") else {
        eprintln!("SKIP test_proposal_lifecycle: proposal_approve fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&create, conn.as_ref()).await?;
    module.process_block(&approve, conn.as_ref()).await?;

    // Load optional on_get_snapshot callback fixture.
    let has_snapshot = try_load_fixture("on_get_snapshot").is_some();
    if let Some(snapshot) = try_load_fixture("on_get_snapshot") {
        module.process_block(&snapshot, conn.as_ref()).await?;
    }

    // near_venear_proposal_events: create_proposal event recorded with proposal_id
    let create_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events
         WHERE event_type = 'create_proposal'",
    )
    .await?;
    assert!(
        create_count >= 1,
        "expected >= 1 create_proposal event row, got {create_count}"
    );

    let create_with_proposal_id = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events
         WHERE event_type = 'create_proposal' AND proposal_id IS NOT NULL",
    )
    .await?;
    assert!(
        create_with_proposal_id >= 1,
        "expected create_proposal rows to have proposal_id set, got {create_with_proposal_id}"
    );

    // near_venear_proposal_events: proposal_approve event recorded
    let approve_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events
         WHERE event_type = 'proposal_approve'",
    )
    .await?;
    assert!(
        approve_count >= 1,
        "expected >= 1 proposal_approve event row, got {approve_count}"
    );

    // near_venear_proposal_events: finalized callback recorded (if snapshot fixture loaded)
    if has_snapshot {
        let finalized_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events
             WHERE event_type = 'proposal_approve_finalized'",
        )
        .await?;
        assert!(
            finalized_count >= 1,
            "expected >= 1 proposal_approve_finalized event row after on_get_snapshot, got {finalized_count}"
        );

        let finalized_with_snapshot = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events
             WHERE event_type = 'proposal_approve_finalized'
               AND reviewer_id IS NOT NULL
               AND snapshot_block_height IS NOT NULL
               AND snapshot_root IS NOT NULL
               AND snapshot_total_venear IS NOT NULL",
        )
        .await?;
        assert!(
            finalized_with_snapshot >= 1,
            "expected finalized callback rows to include reviewer and snapshot fields, got {finalized_with_snapshot}"
        );

        let finalized_with_metadata = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events
             WHERE event_type = 'proposal_approve_finalized'
               AND metadata->>'title' IS NOT NULL
               AND metadata->>'status' IS NOT NULL",
        )
        .await?;
        assert!(
            finalized_with_metadata >= 1,
            "expected finalized callback metadata (title/status) to be populated, got {finalized_with_metadata}"
        );

        let finalized_with_success_value = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events
             WHERE event_type = 'proposal_approve_finalized'
               AND payload->'success_value' IS NOT NULL",
        )
        .await?;
        assert!(
            finalized_with_success_value >= 1,
            "expected finalized callback payload to include decoded success_value, got {finalized_with_success_value}"
        );
    }

    // near_venear_events_raw: create_proposal and proposal_approve events recorded
    let raw_create = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
         WHERE event_type = 'create_proposal' AND standard = 'venear'",
    )
    .await?;
    assert!(
        raw_create >= 1,
        "expected >= 1 raw event row with event_type='create_proposal', got {raw_create}"
    );

    let raw_approve = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
         WHERE event_type = 'proposal_approve' AND standard = 'venear'",
    )
    .await?;
    assert!(
        raw_approve >= 1,
        "expected >= 1 raw event row with event_type='proposal_approve', got {raw_approve}"
    );

    // near_venear_votes: must be empty (no voting in this test)
    let vote_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes",
    )
    .await?;
    assert_eq!(
        vote_count, 0,
        "expected 0 vote rows from proposal lifecycle test, got {vote_count}"
    );

    // near_venear_lockup_actions: must be empty (no lockup calls in proposal blocks)
    let action_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_actions",
    )
    .await?;
    assert_eq!(
        action_count, 0,
        "expected 0 lockup_action rows from proposal lifecycle test, got {action_count}"
    );

    Ok(())
}

/// Proposal rejection: create → reject.
#[tokio::test]
async fn test_proposal_reject() -> Result<()> {
    init_test_tracing();

    let Some(create) = try_load_fixture("create_proposal") else {
        eprintln!("SKIP test_proposal_reject: create_proposal fixture not found");
        return Ok(());
    };
    let Some(reject) = try_load_fixture("proposal_reject") else {
        eprintln!("SKIP test_proposal_reject: proposal_reject fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&create, conn.as_ref()).await?;
    module.process_block(&reject, conn.as_ref()).await?;

    // near_venear_proposal_events: proposal_reject event recorded
    let reject_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events
         WHERE event_type = 'proposal_reject'",
    )
    .await?;
    assert!(
        reject_count >= 1,
        "expected >= 1 proposal_reject event row, got {reject_count}"
    );

    // near_venear_events_raw: proposal_reject event recorded
    let raw_reject = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
         WHERE event_type = 'proposal_reject' AND standard = 'venear'",
    )
    .await?;
    assert!(
        raw_reject >= 1,
        "expected >= 1 raw event row with event_type='proposal_reject', got {raw_reject}"
    );

    // near_venear_votes: must be empty (rejected proposals have no votes)
    let vote_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes",
    )
    .await?;
    assert_eq!(
        vote_count, 0,
        "expected 0 vote rows from proposal_reject test, got {vote_count}"
    );

    Ok(())
}

/// Voting: add_vote creates a row in near_venear_votes with proposal_id, account_balance.
/// Tables NOT affected: near_venear_proposal_events, near_venear_lockup_actions.
#[tokio::test]
async fn test_voting() -> Result<()> {
    init_test_tracing();

    let Some(add) = try_load_fixture("add_vote") else {
        eprintln!("SKIP test_voting: add_vote fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&add, conn.as_ref()).await?;

    // near_venear_votes: add_vote row with required fields populated
    let add_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes
         WHERE event_type = 'add_vote'",
    )
    .await?;
    assert!(
        add_count >= 1,
        "expected >= 1 add_vote row, got {add_count}"
    );

    let vote_with_fields = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes
         WHERE event_type = 'add_vote'
           AND proposal_id IS NOT NULL
           AND account_id IS NOT NULL
           AND account_balance IS NOT NULL",
    )
    .await?;
    assert!(
        vote_with_fields >= 1,
        "expected add_vote rows to have proposal_id, account_id, and account_balance set, got {vote_with_fields}"
    );

    let voting_power_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_voting_power_events
         WHERE event_type = 'add_vote'",
    )
    .await?;
    assert!(
        voting_power_count >= 1,
        "expected >= 1 account_voting_power add_vote row, got {voting_power_count}"
    );

    // near_venear_events_raw: add_vote event recorded with venear standard
    let raw_vote = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
         WHERE event_type = 'add_vote' AND standard = 'venear'",
    )
    .await?;
    assert!(
        raw_vote >= 1,
        "expected >= 1 raw event row with event_type='add_vote', got {raw_vote}"
    );

    // near_venear_proposal_events: must be empty (add_vote block has no proposal events)
    let proposal_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events",
    )
    .await?;
    assert_eq!(
        proposal_count, 0,
        "expected 0 proposal events from add_vote block, got {proposal_count}"
    );

    // near_venear_lockup_actions: must be empty (add_vote block has no lockup calls)
    let action_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_actions",
    )
    .await?;
    assert_eq!(
        action_count, 0,
        "expected 0 lockup_action rows from add_vote block, got {action_count}"
    );

    // Optional remove_vote
    if let Some(remove) = try_load_fixture("remove_vote") {
        module.process_block(&remove, conn.as_ref()).await?;

        let remove_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes
             WHERE event_type = 'remove_vote'",
        )
        .await?;
        assert!(
            remove_count >= 1,
            "expected >= 1 remove_vote row, got {remove_count}"
        );
    }

    Ok(())
}

/// ft_mint increases cumulative balance; ft_burn decreases it (negative delta_amount).
/// ft events must come from venear.dao (nep141 standard).
/// Tables NOT affected: near_venear_proposal_events, near_venear_votes, near_venear_lockup_actions.
#[tokio::test]
async fn test_ft_mint_and_burn() -> Result<()> {
    init_test_tracing();

    let Some(mint) = try_load_fixture("ft_mint") else {
        eprintln!("SKIP test_ft_mint_and_burn: ft_mint fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&mint, conn.as_ref()).await?;

    // near_venear_account_balances: ft_mint row with positive delta_amount
    let mint_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances
         WHERE event_type = 'ft_mint'",
    )
    .await?;
    assert!(
        mint_count >= 1,
        "expected >= 1 ft_mint balance row, got {mint_count}"
    );

    let mint_positive_delta = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances
         WHERE event_type = 'ft_mint'
           AND delta_amount IS NOT NULL
           AND delta_amount NOT LIKE '-%'",
    )
    .await?;
    assert!(
        mint_positive_delta >= 1,
        "expected ft_mint delta_amount to be positive (not starting with '-'), got {mint_positive_delta}"
    );

    // near_venear_account_balances: contract_id must be venear.dao
    let mint_from_venear = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances
         WHERE event_type = 'ft_mint' AND contract_id = 'venear.dao'",
    )
    .await?;
    assert!(
        mint_from_venear >= 1,
        "expected ft_mint events to come from venear.dao, got {mint_from_venear} rows from venear.dao"
    );

    // near_venear_events_raw: ft_mint event with nep141 standard
    let raw_mint = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
         WHERE event_type = 'ft_mint' AND standard = 'nep141'",
    )
    .await?;
    assert!(
        raw_mint >= 1,
        "expected >= 1 raw event row with event_type='ft_mint' and standard='nep141', got {raw_mint}"
    );

    let mint_voting_power_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_voting_power_events
         WHERE event_type = 'ft_mint'",
    )
    .await?;
    assert!(
        mint_voting_power_count >= 1,
        "expected >= 1 account_voting_power ft_mint row, got {mint_voting_power_count}"
    );

    // near_venear_proposal_events: must be empty (ft_mint block has no proposals)
    let proposal_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events",
    )
    .await?;
    assert_eq!(
        proposal_count, 0,
        "expected 0 proposal events from ft_mint block, got {proposal_count}"
    );

    // near_venear_votes: must be empty
    let vote_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes",
    )
    .await?;
    assert_eq!(
        vote_count, 0,
        "expected 0 vote rows from ft_mint block, got {vote_count}"
    );

    // near_venear_lockup_actions: must be empty (ft_mint block has no lockup calls)
    let action_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_actions",
    )
    .await?;
    assert_eq!(
        action_count, 0,
        "expected 0 lockup_action rows from ft_mint block, got {action_count}"
    );

    // Optional ft_burn: negative delta_amount
    if let Some(burn) = try_load_fixture("ft_burn") {
        module.process_block(&burn, conn.as_ref()).await?;

        let burn_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances
             WHERE event_type = 'ft_burn'",
        )
        .await?;
        assert!(
            burn_count >= 1,
            "expected >= 1 ft_burn balance row, got {burn_count}"
        );

        let burn_negative_delta = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances
             WHERE event_type = 'ft_burn'
               AND delta_amount IS NOT NULL
               AND delta_amount LIKE '-%'",
        )
        .await?;
        assert!(
            burn_negative_delta >= 1,
            "expected ft_burn delta_amount to be negative (starting with '-'), got {burn_negative_delta}"
        );

        // near_venear_events_raw: ft_burn event with nep141 standard
        let raw_burn = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
             WHERE event_type = 'ft_burn' AND standard = 'nep141'",
        )
        .await?;
        assert!(
            raw_burn >= 1,
            "expected >= 1 raw event row with event_type='ft_burn', got {raw_burn}"
        );

        let burn_voting_power_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_voting_power_events
             WHERE event_type = 'ft_burn'",
        )
        .await?;
        assert!(
            burn_voting_power_count >= 1,
            "expected >= 1 account_voting_power ft_burn row, got {burn_voting_power_count}"
        );
    }

    Ok(())
}

/// on_lockup_update callback → lockup row in near_venear_lockup_events.
/// The on_lockup_update fixture (block 167957476) also contains a lockup_update event,
/// so near_venear_account_balances is expected to have rows too.
#[tokio::test]
async fn test_on_lockup_update_callback() -> Result<()> {
    init_test_tracing();

    let Some(fixture) = try_load_fixture("on_lockup_update") else {
        eprintln!("SKIP test_on_lockup_update_callback: on_lockup_update fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&fixture, conn.as_ref()).await?;

    // near_venear_lockup_events: lockup callback event row with correct suffix
    let lockup_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events",
    )
    .await?;
    assert!(
        lockup_count >= 1,
        "expected >= 1 lockup row after on_lockup_update callback, got {lockup_count}"
    );

    let lockup_with_suffix = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE lockup_account_id LIKE '%.venear.dao'",
    )
    .await?;
    assert!(
        lockup_with_suffix >= 1,
        "expected >= 1 .venear.dao lockup event after on_lockup_update, got {lockup_with_suffix}"
    );

    // near_venear_account_balances: lockup_update event in same block
    let balance_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances
         WHERE event_type = 'lockup_update'",
    )
    .await?;
    assert!(
        balance_count >= 1,
        "expected >= 1 lockup_update balance row (same block as on_lockup_update), got {balance_count}"
    );

    // near_venear_events_raw: lockup_update event in same block generates a raw row
    // (on_lockup_update is a callback, not an EVENT_JSON log, so no raw event for it)
    let raw_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
         WHERE event_type = 'lockup_update'",
    )
    .await?;
    assert!(
        raw_count >= 1,
        "expected >= 1 raw event row from lockup_update in same block, got {raw_count}"
    );

    // near_venear_proposal_events: must be empty
    let proposal_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events",
    )
    .await?;
    assert_eq!(
        proposal_count, 0,
        "expected 0 proposal events from on_lockup_update block, got {proposal_count}"
    );

    // near_venear_votes: must be empty
    let vote_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes",
    )
    .await?;
    assert_eq!(
        vote_count, 0,
        "expected 0 vote rows from on_lockup_update block, got {vote_count}"
    );

    Ok(())
}

/// Lockup actions → rows in near_venear_lockup_actions for known lockups.
/// Pre-load lockup_deployed to prime the module's known-lockup set.
/// Tables NOT affected: near_venear_proposal_events, near_venear_votes.
#[tokio::test]
async fn test_lockup_action() -> Result<()> {
    init_test_tracing();

    let Some(fixture) = try_load_fixture("lockup_action") else {
        eprintln!("SKIP test_lockup_action: lockup_action fixture not found");
        return Ok(());
    };

    // Pre-load the deployed lockup so the module's known_lockup set is primed from DB.
    let deployed = try_load_fixture("lockup_deployed");

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    if let Some(deployed) = deployed {
        module.process_block(&deployed, conn.as_ref()).await?;
    }

    module.process_block(&fixture, conn.as_ref()).await?;

    // near_venear_lockup_actions: rows with .venear.dao lockup account
    let action_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_actions",
    )
    .await?;
    assert!(
        action_count >= 1,
        "expected >= 1 lockup_action row, got {action_count}"
    );

    let action_with_suffix = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_actions
         WHERE lockup_account_id LIKE '%.venear.dao'",
    )
    .await?;
    assert!(
        action_with_suffix >= 1,
        "expected lockup_action rows to have lockup_account_id ending in .venear.dao, got {action_with_suffix}"
    );

    // near_venear_events_raw: all function-call actions on target contracts are captured.
    let raw_method_rows = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
         WHERE record_kind = 'method_call'",
    )
    .await?;
    assert!(
        raw_method_rows >= 1,
        "expected >= 1 raw method_call row, got {raw_method_rows}"
    );

    // near_venear_lockup_events: lockup_deployed pre-load created a row
    let lockup_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE event_type = 'lockup_deployed'",
    )
    .await?;
    assert!(
        lockup_count >= 1,
        "expected >= 1 lockup_deployed row (from lockup_deployed pre-load), got {lockup_count}"
    );

    // near_venear_proposal_events: must be empty (no proposals in either block)
    let proposal_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events",
    )
    .await?;
    assert_eq!(
        proposal_count, 0,
        "expected 0 proposal events from lockup_action test, got {proposal_count}"
    );

    // near_venear_votes: must be empty
    let vote_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes",
    )
    .await?;
    assert_eq!(
        vote_count, 0,
        "expected 0 vote rows from lockup_action test, got {vote_count}"
    );

    Ok(())
}

/// Delegation actions on venear.dao (delegate_all / undelegate) are indexed
/// into near_venear_delegation_events.
#[tokio::test]
async fn test_delegation_actions_success() -> Result<()> {
    init_test_tracing();

    let Some(delegate_all) = try_load_fixture("delegate_all") else {
        eprintln!("SKIP test_delegation_actions_success: delegate_all fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&delegate_all, conn.as_ref()).await?;

    let delegate_all_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_delegation_events
         WHERE event_type = 'delegate_all' AND success = true",
    )
    .await?;
    assert!(
        delegate_all_count >= 1,
        "expected >= 1 successful delegate_all row, got {delegate_all_count}"
    );

    let delegatee_present = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_delegation_events
         WHERE event_type = 'delegate_all' AND delegatee_account_id IS NOT NULL",
    )
    .await?;
    assert!(
        delegatee_present >= 1,
        "expected delegate_all row to have delegatee_account_id, got {delegatee_present}"
    );

    if let Some(undelegate) = try_load_fixture("undelegate") {
        module.process_block(&undelegate, conn.as_ref()).await?;

        let undelegate_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_delegation_events
             WHERE event_type = 'undelegate' AND success = true",
        )
        .await?;
        assert!(
            undelegate_count >= 1,
            "expected >= 1 successful undelegate row, got {undelegate_count}"
        );

        let undelegate_without_delegatee = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_delegation_events
             WHERE event_type = 'undelegate' AND delegatee_account_id IS NULL",
        )
        .await?;
        assert!(
            undelegate_without_delegatee >= 1,
            "expected undelegate row with null delegatee_account_id, got {undelegate_without_delegatee}"
        );
    }

    Ok(())
}

/// Real-chain delegation lifecycle:
/// A delegates to B, then A reduces lockup balance, and B's voting power is burned.
#[tokio::test]
async fn test_delegation_then_lockup_reduction_burns_delegatee_voting_power() -> Result<()> {
    init_test_tracing();

    let delegate_block = load_fixture("delegate_all_horbarr_to_sabrinanear");
    let begin_unlock_block = load_fixture("begin_unlock_near_horbarr_after_delegate");
    let callback_block = load_fixture("on_lockup_update_delegatee_burn_horbarr");

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&delegate_block, conn.as_ref()).await?;
    module
        .process_block(&begin_unlock_block, conn.as_ref())
        .await?;
    module.process_block(&callback_block, conn.as_ref()).await?;

    let delegation_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_venear_delegation_events
         WHERE event_type = 'delegate_all'
           AND success = true
           AND delegator_account_id = 'horbarr.near'
           AND delegatee_account_id = 'sabrinanear.near'",
    )
    .await?;
    assert_eq!(
        delegation_count, 1,
        "expected exactly one successful delegate_all(horbarr->sabrinanear), got {delegation_count}"
    );

    let begin_unlock_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_venear_lockup_events
         WHERE event_type = 'begin_unlock_near'
           AND lockup_account_id = 'b155f46a0ec06336370ab6a9b26607779df26b9d.venear.dao'",
    )
    .await?;
    assert!(
        begin_unlock_count >= 1,
        "expected begin_unlock_near lockup event for horbarr lockup, got {begin_unlock_count}"
    );

    let lockup_update_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_venear_account_balances
         WHERE receipt_id = 'FPNphxYAxh76SsPdTumDSQJfYbHEe5fWVHE2juvo551Z'
           AND event_type = 'lockup_update'
           AND account_id = 'horbarr.near'
           AND locked_near_balance = '1'",
    )
    .await?;
    assert_eq!(
        lockup_update_count, 1,
        "expected one lockup_update row for horbarr in callback receipt"
    );

    let delegatee_burn_balance_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_venear_account_balances
         WHERE receipt_id = 'FPNphxYAxh76SsPdTumDSQJfYbHEe5fWVHE2juvo551Z'
           AND event_type = 'ft_burn'
           AND account_id = 'sabrinanear.near'
           AND delta_amount = '-5000016432221189300000000'",
    )
    .await?;
    assert_eq!(
        delegatee_burn_balance_count, 1,
        "expected one delegatee ft_burn balance row after lockup reduction"
    );

    let delegatee_burn_voting_power_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_venear_account_voting_power_events
         WHERE receipt_id = 'FPNphxYAxh76SsPdTumDSQJfYbHEe5fWVHE2juvo551Z'
           AND event_type = 'ft_burn'
           AND account_id = 'sabrinanear.near'
           AND delta_voting_power = '-5000016432221189300000000'",
    )
    .await?;
    assert_eq!(
        delegatee_burn_voting_power_count, 1,
        "expected one delegatee ft_burn voting-power row after lockup reduction"
    );

    let delegator_burn_voting_power_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_venear_account_voting_power_events
         WHERE receipt_id = 'FPNphxYAxh76SsPdTumDSQJfYbHEe5fWVHE2juvo551Z'
           AND event_type = 'ft_burn'
           AND account_id = 'horbarr.near'",
    )
    .await?;
    assert_eq!(
        delegator_burn_voting_power_count, 0,
        "expected no ft_burn voting-power row for delegator in callback receipt"
    );

    Ok(())
}

/// Failed delegation actions are retained with success=false.
#[tokio::test]
async fn test_delegation_actions_failures() -> Result<()> {
    init_test_tracing();

    let delegate_all_failed = try_load_fixture("delegate_all_failed");
    let undelegate_failed = try_load_fixture("undelegate_failed");

    if delegate_all_failed.is_none() && undelegate_failed.is_none() {
        eprintln!(
            "SKIP test_delegation_actions_failures: delegate_all_failed and undelegate_failed fixtures not found"
        );
        return Ok(());
    }

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    if let Some(fixture) = delegate_all_failed {
        module.process_block(&fixture, conn.as_ref()).await?;
    }
    if let Some(fixture) = undelegate_failed {
        module.process_block(&fixture, conn.as_ref()).await?;
    }

    let failed_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_delegation_events
         WHERE success = false",
    )
    .await?;
    assert!(
        failed_count >= 1,
        "expected >= 1 failed delegation row, got {failed_count}"
    );

    Ok(())
}

/// Admin methods on venear.dao / vote.dao are captured in near_venear_admin_events
/// and also mirrored into near_venear_events_raw as method_call rows.
#[tokio::test]
async fn test_admin_methods_capture() -> Result<()> {
    init_test_tracing();

    let fixture_specs = [
        (
            "admin_set_lockup_contract",
            "venear.dao",
            "set_lockup_contract",
        ),
        (
            "admin_set_lockup_code_deployers",
            "venear.dao",
            "set_lockup_code_deployers",
        ),
        (
            "admin_prepare_lockup_code",
            "venear.dao",
            "prepare_lockup_code",
        ),
        ("admin_set_guardians", "vote.dao", "set_guardians"),
        ("admin_set_reviewer_ids", "vote.dao", "set_reviewer_ids"),
        ("admin_upgrade", "vote.dao", "upgrade"),
        ("admin_migrate_state", "vote.dao", "migrate_state"),
    ];

    let available: Vec<(&str, &str, &str, StreamerMessage)> = fixture_specs
        .iter()
        .filter_map(|(fixture_name, contract_id, method_name)| {
            try_load_fixture(fixture_name)
                .map(|fixture| (*fixture_name, *contract_id, *method_name, fixture))
        })
        .collect();

    if available.is_empty() {
        eprintln!("SKIP test_admin_methods_capture: admin fixtures not found");
        return Ok(());
    }

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    for (fixture_name, _, _, fixture) in &available {
        module
            .process_block(fixture, conn.as_ref())
            .await
            .unwrap_or_else(|e| panic!("process_block failed for fixture '{fixture_name}': {e}"));
    }

    for (fixture_name, contract_id, method_name, _) in &available {
        let admin_count = scalar_i64(
            conn.as_ref(),
            &format!(
                "SELECT COUNT(*)::bigint AS value FROM near.near_venear_admin_events\n                 WHERE contract_id = '{}' AND method_name = '{}'",
                contract_id, method_name
            ),
        )
        .await?;
        assert!(
            admin_count >= 1,
            "expected >= 1 admin event row for fixture {} ({contract_id}.{method_name}), got {admin_count}",
            fixture_name
        );

        let raw_method_count = scalar_i64(
            conn.as_ref(),
            &format!(
                "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw\n                 WHERE record_kind = 'method_call' AND contract_id = '{}' AND method_name = '{}'",
                contract_id, method_name
            ),
        )
        .await?;
        assert!(
            raw_method_count >= 1,
            "expected >= 1 raw method_call row for fixture {} ({contract_id}.{method_name}), got {raw_method_count}",
            fixture_name
        );
    }

    Ok(())
}

/// Optional coverage for plain non-EVENT_JSON logs on target contracts.
#[tokio::test]
async fn test_plain_logs_capture_if_available() -> Result<()> {
    init_test_tracing();

    let Some(fixture) = try_load_fixture("plain_log") else {
        eprintln!("SKIP test_plain_logs_capture_if_available: plain_log fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&fixture, conn.as_ref()).await?;

    let plain_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw
         WHERE record_kind = 'plain_log'",
    )
    .await?;
    assert!(
        plain_count >= 1,
        "expected >= 1 plain_log row in near_venear_events_raw, got {plain_count}"
    );

    Ok(())
}

/// Idempotency: re-processing the same delegation fixture does not duplicate rows.
#[tokio::test]
async fn test_delegation_idempotency() -> Result<()> {
    init_test_tracing();

    let Some(fixture) = try_load_fixture("delegate_all") else {
        eprintln!("SKIP test_delegation_idempotency: delegate_all fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&fixture, conn.as_ref()).await?;
    let count_first = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_delegation_events",
    )
    .await?;

    module.process_block(&fixture, conn.as_ref()).await?;
    let count_second = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_delegation_events",
    )
    .await?;

    assert_eq!(
        count_first, count_second,
        "re-processing delegation fixture must not create duplicate rows"
    );

    Ok(())
}

/// Idempotency: processing the same fixture twice does not create duplicate rows.
#[tokio::test]
async fn test_ft_mint_idempotency() -> Result<()> {
    init_test_tracing();

    let Some(fixture) = try_load_fixture("ft_mint") else {
        eprintln!("SKIP test_ft_mint_idempotency: ft_mint fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&fixture, conn.as_ref()).await?;
    let count_first = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances
         WHERE event_type = 'ft_mint'",
    )
    .await?;

    // Process the same block a second time.
    module.process_block(&fixture, conn.as_ref()).await?;
    let count_second = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances
         WHERE event_type = 'ft_mint'",
    )
    .await?;

    assert_eq!(
        count_first, count_second,
        "re-processing the same block must not create duplicate rows (idempotent)"
    );

    Ok(())
}

/// Idempotency for raw events table.
#[tokio::test]
async fn test_raw_events_idempotency() -> Result<()> {
    init_test_tracing();

    // Use any available fixture.
    let fixture = try_load_fixture("lockup_deployed")
        .or_else(|| try_load_fixture("ft_mint"))
        .or_else(|| try_load_fixture("create_proposal"));

    let Some(fixture) = fixture else {
        eprintln!("SKIP test_raw_events_idempotency: no fixture available");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&fixture, conn.as_ref()).await?;
    let count_first = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw",
    )
    .await?;

    module.process_block(&fixture, conn.as_ref()).await?;
    let count_second = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw",
    )
    .await?;

    assert_eq!(
        count_first, count_second,
        "raw events table must be idempotent on re-processing"
    );

    Ok(())
}

/// Smoke test: all available fixtures can be processed without errors.
/// Also verifies that each relevant table has at least one row after processing all fixtures.
#[tokio::test]
async fn test_all_available_fixtures_process_cleanly() -> Result<()> {
    init_test_tracing();

    let all_names = [
        "lockup_deployed",
        "lockup_update",
        "lockup_delete",
        "lockup_lock_near",
        "create_proposal",
        "proposal_approve",
        "proposal_reject",
        "on_get_snapshot",
        "add_vote",
        "remove_vote",
        "ft_mint",
        "ft_burn",
        "on_lockup_update",
        "on_lockup_deployed",
        "on_lockup_deployed_failed",
        "lockup_action",
        "begin_unlock_near",
        "begin_unlock_near_horbarr_after_delegate",
        "end_unlock_near",
        "lock_pending_near",
        "delegate_all",
        "delegate_all_horbarr_to_sabrinanear",
        "undelegate",
        "delegate_all_failed",
        "undelegate_failed",
        "on_lockup_update_delegatee_burn_horbarr",
        "admin_set_lockup_contract",
        "admin_set_lockup_code_deployers",
        "admin_prepare_lockup_code",
        "admin_set_guardians",
        "admin_set_reviewer_ids",
        "admin_upgrade",
        "admin_migrate_state",
        "plain_log",
    ];

    let fixtures: Vec<(&str, StreamerMessage)> = all_names
        .iter()
        .filter_map(|&name| try_load_fixture(name).map(|f| (name, f)))
        .collect();

    if fixtures.is_empty() {
        eprintln!("SKIP test_all_available_fixtures_process_cleanly: no fixtures found");
        return Ok(());
    }

    let loaded_names: std::collections::HashSet<&str> =
        fixtures.iter().map(|(name, _)| *name).collect();

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    for (name, fixture) in &fixtures {
        module
            .process_block(fixture, conn.as_ref())
            .await
            .unwrap_or_else(|e| panic!("process_block failed for fixture '{name}': {e}"));
    }

    // near_venear_events_raw: at least one row per distinct block for event-bearing fixtures.
    // Action-only fixtures may not contain EVENT_JSON logs in the same block.
    let raw_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw",
    )
    .await?;

    let distinct_blocks: std::collections::HashSet<u64> = fixtures
        .iter()
        .map(|(_, f)| f.block.header.height)
        .collect();

    let action_only_fixtures: std::collections::HashSet<&str> = [
        "lockup_action",
        "on_lockup_deployed_failed",
        "delegate_all",
        "undelegate",
        "delegate_all_failed",
        "undelegate_failed",
    ]
    .into_iter()
    .collect();

    let expected_raw_blocks: std::collections::HashSet<u64> = fixtures
        .iter()
        .filter(|(name, _)| !action_only_fixtures.contains(*name))
        .map(|(_, f)| f.block.header.height)
        .collect();

    assert!(
        raw_count >= expected_raw_blocks.len() as i64,
        "expected at least {} raw event rows (one per event-bearing fixture block), got {}",
        expected_raw_blocks.len(),
        raw_count
    );

    // near_venear_lockup_events: populated by lockup_deployed and/or on_lockup_update
    if loaded_names.contains("lockup_deployed") || loaded_names.contains("on_lockup_update") {
        let lockup_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events",
        )
        .await?;
        assert!(
            lockup_count >= 1,
            "expected >= 1 row in near_venear_lockup_events after loading lockup fixtures, got {lockup_count}"
        );
    }

    // near_venear_account_balances: populated by lockup_update, ft_mint, ft_burn
    if loaded_names.contains("lockup_update")
        || loaded_names.contains("ft_mint")
        || loaded_names.contains("ft_burn")
    {
        let balance_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances",
        )
        .await?;
        assert!(
            balance_count >= 1,
            "expected >= 1 row in near_venear_account_balances after loading balance fixtures, got {balance_count}"
        );
    }

    // near_venear_account_voting_power_events: populated by add_vote/remove_vote and balance events.
    if loaded_names.contains("add_vote")
        || loaded_names.contains("remove_vote")
        || loaded_names.contains("ft_mint")
        || loaded_names.contains("ft_burn")
        || loaded_names.contains("lockup_update")
    {
        let voting_power_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_voting_power_events",
        )
        .await?;
        assert!(
            voting_power_count >= 1,
            "expected >= 1 row in near_venear_account_voting_power_events after loading voting-power fixtures, got {voting_power_count}"
        );
    }

    // near_venear_proposal_events: populated by create_proposal, proposal_approve, on_get_snapshot
    if loaded_names.contains("create_proposal")
        || loaded_names.contains("proposal_approve")
        || loaded_names.contains("on_get_snapshot")
    {
        let proposal_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events",
        )
        .await?;
        assert!(
            proposal_count >= 1,
            "expected >= 1 row in near_venear_proposal_events after loading proposal fixtures, got {proposal_count}"
        );
    }

    // near_venear_votes: populated by add_vote, remove_vote
    if loaded_names.contains("add_vote") || loaded_names.contains("remove_vote") {
        let vote_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes",
        )
        .await?;
        assert!(
            vote_count >= 1,
            "expected >= 1 row in near_venear_votes after loading vote fixtures, got {vote_count}"
        );
    }

    // near_venear_lockup_actions: populated by lockup_action (when lockup is known)
    // lockup_deployed is also loaded so the lockup is registered before lockup_action block
    if loaded_names.contains("lockup_action") && loaded_names.contains("lockup_deployed") {
        let action_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_actions",
        )
        .await?;
        assert!(
            action_count >= 1,
            "expected >= 1 row in near_venear_lockup_actions after loading lockup_action fixture, got {action_count}"
        );
    }

    if loaded_names.contains("delegate_all")
        || loaded_names.contains("undelegate")
        || loaded_names.contains("delegate_all_failed")
        || loaded_names.contains("undelegate_failed")
    {
        let delegation_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_delegation_events",
        )
        .await?;
        assert!(
            delegation_count >= 1,
            "expected >= 1 row in near_venear_delegation_events after loading delegation fixtures, got {delegation_count}"
        );
    }

    if loaded_names.contains("admin_set_lockup_contract")
        || loaded_names.contains("admin_set_lockup_code_deployers")
        || loaded_names.contains("admin_prepare_lockup_code")
        || loaded_names.contains("admin_set_guardians")
        || loaded_names.contains("admin_set_reviewer_ids")
        || loaded_names.contains("admin_upgrade")
        || loaded_names.contains("admin_migrate_state")
    {
        let admin_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_admin_events",
        )
        .await?;
        assert!(
            admin_count >= 1,
            "expected >= 1 row in near.near_venear_admin_events after loading admin fixtures, got {admin_count}"
        );
    }

    // near_venear_lockup_events: populated by unlock lifecycle methods
    if loaded_names.contains("begin_unlock_near")
        || loaded_names.contains("end_unlock_near")
        || loaded_names.contains("lock_pending_near")
    {
        let unlock_event_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
             WHERE event_type IN ('begin_unlock_near', 'end_unlock_near', 'lock_pending_near')",
        )
        .await?;
        assert!(
            unlock_event_count >= 1,
            "expected >= 1 lockup_event row for unlock lifecycle methods, got {unlock_event_count}"
        );
    }

    // ---------------------------------------------------------------------
    // Value-level fixture assertions for all known paths.
    // ---------------------------------------------------------------------

    if loaded_names.contains("lockup_deployed") {
        let deployed_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_lockup_events
             WHERE event_type = 'lockup_deployed'
               AND receipt_id = 'uQFKagNeyRnjen2jt5XzWcGsee1qFuFsTWDGAdwbDzg'
               AND lockup_account_id = '0887fe221cafb6810909c83393e23585c439fe99.venear.dao'
               AND owner_account_id = 'jchos.near'
               AND lockup_version = 1
               AND predecessor_account_id = 'venear.dao'
               AND contract_id = 'venear.dao'
               AND success = true",
        )
        .await?;
        assert_eq!(deployed_exact, 1, "lockup_deployed fixture values mismatch");

        let deployed_callback_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_lockup_events
             WHERE event_type = 'lockup_deployed_callback'
               AND receipt_id = 'uQFKagNeyRnjen2jt5XzWcGsee1qFuFsTWDGAdwbDzg'
               AND lockup_account_id = '0887fe221cafb6810909c83393e23585c439fe99.venear.dao'
               AND owner_account_id = 'jchos.near'
               AND lockup_version = 1
               AND predecessor_account_id = 'venear.dao'
               AND contract_id = 'venear.dao'
               AND action_source = 'receipt'
               AND success = true",
        )
        .await?;
        assert_eq!(
            deployed_callback_exact, 1,
            "lockup_deployed callback values mismatch"
        );

        let raw_event_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_events_raw
             WHERE record_kind = 'event_json'
               AND execution_outcome_id = 'uQFKagNeyRnjen2jt5XzWcGsee1qFuFsTWDGAdwbDzg'
               AND standard = 'venear'
               AND event_type = 'lockup_deployed'
               AND predecessor_account_id = 'venear.dao'
               AND contract_id = 'venear.dao'
               AND success = true
               AND event_data->>'account_id' = 'jchos.near'",
        )
        .await?;
        assert_eq!(raw_event_exact, 1, "raw lockup_deployed row mismatch");
    }

    if loaded_names.contains("lockup_update") {
        let update_balance_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_account_balances
             WHERE receipt_id = '6C1GEUFnG3kno1NiGK4yHKeCPbgXeZk6YT1w2gJEB2E3'
               AND event_type = 'lockup_update'
               AND account_id = 'jchos.near'
               AND total_balance = '0'
               AND locked_near_balance = '2000016409706328300000001'
               AND lockup_update_nonce = 167957455000001
               AND lockup_version = 1",
        )
        .await?;
        assert_eq!(update_balance_exact, 1, "lockup_update balance mismatch");

        let update_callback_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_lockup_events
             WHERE receipt_id = '6C1GEUFnG3kno1NiGK4yHKeCPbgXeZk6YT1w2gJEB2E3'
               AND event_type = 'lockup_update_callback'
               AND lockup_account_id = '0887fe221cafb6810909c83393e23585c439fe99.venear.dao'
               AND owner_account_id = 'jchos.near'
               AND lockup_version = 1
               AND lockup_update_nonce IS NULL
               AND locked_near_balance IS NULL
               AND action_source = 'receipt'
               AND predecessor_account_id = '0887fe221cafb6810909c83393e23585c439fe99.venear.dao'
               AND contract_id = 'venear.dao'
               AND success = true
               AND payload->>'version' = '1'
               AND payload->'update'->'V1'->>'lockup_update_nonce' = '167957455000001'
               AND payload->'update'->'V1'->>'locked_near_balance' = '2000016409706328300000001'",
        )
        .await?;
        assert_eq!(update_callback_exact, 1, "lockup_update callback mismatch");
    }

    if loaded_names.contains("lockup_delete") {
        let delete_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_lockup_events
             WHERE receipt_id = '3sJLfbrj6pLaTQqiSSoPoNpjxKJsoAdWu6qyNgNPf26w'
               AND event_type = 'lockup_delete'
               AND lockup_account_id = '7a4642f807c6eded491bbd5678db6a07e43b95e8.venear.dao'
               AND owner_account_id = 'chikatia.tg'
               AND lockup_version = 1
               AND lockup_update_nonce = 169668284000004
               AND predecessor_account_id = 'chikatia.tg'
               AND contract_id = '7a4642f807c6eded491bbd5678db6a07e43b95e8.venear.dao'",
        )
        .await?;
        assert_eq!(delete_exact, 1, "lockup_delete fixture values mismatch");
    }

    if loaded_names.contains("lockup_lock_near") {
        let lockup_lock_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_lockup_events
             WHERE receipt_id = 'YQrweDswg8hJKnkR8wB2TtQ2dv8XSWezn7C4aHa5tGy'
               AND event_type = 'lockup_lock_near'
               AND lockup_account_id = '0887fe221cafb6810909c83393e23585c439fe99.venear.dao'
               AND owner_account_id = 'jchos.near'
               AND lockup_version = 1
               AND lockup_update_nonce = 167957455000005
               AND locked_near_balance = '100000000000000000000'",
        )
        .await?;
        assert_eq!(
            lockup_lock_exact, 1,
            "lockup_lock_near fixture values mismatch"
        );
    }

    if loaded_names.contains("create_proposal") {
        let create_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_proposal_events
             WHERE receipt_id = 'urM2AvHkXnvWFk4GoxFQ4ifH2nb4VHtkErgQ6PtrgFF'
               AND event_type = 'create_proposal'
               AND proposal_id = 0
               AND proposer_id = 'jchos.near'
               AND metadata->>'title' = 'Test'
               AND metadata->>'description' = 'Test'
               AND metadata->>'link' = 'https://gov.near.org/proposal'",
        )
        .await?;
        assert_eq!(create_exact, 1, "create_proposal fixture values mismatch");
    }

    if loaded_names.contains("proposal_approve") {
        let approve_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_proposal_events
             WHERE receipt_id = '94SuRmTkV7J6LsycGNwFyrLz34d2t5ephwb5tK2Ppqxr'
               AND event_type = 'proposal_approve'
               AND proposal_id = 2
               AND account_id = 'lane.near'
               AND reviewer_id = 'lane.near'",
        )
        .await?;
        assert_eq!(approve_exact, 1, "proposal_approve fixture values mismatch");
    }

    if loaded_names.contains("on_get_snapshot") {
        let finalized_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_proposal_events
             WHERE receipt_id = '9m6gwp2xhA3dHqTTo6wX4ZSefb9Nj8d7CPV5g9RPSF97'
               AND event_type = 'proposal_approve_finalized'
               AND proposal_id = 2
               AND reviewer_id = 'lane.near'
               AND snapshot_block_height = 168368385
               AND snapshot_root = '6XLjZFf5MgZxXEkcHVjrT5FDDM1Q3k9zpdLZrrsjDW1Y'
               AND snapshot_total_venear = '33557888548819293211318846044'
               AND metadata->>'status' = 'Voting'",
        )
        .await?;
        assert_eq!(
            finalized_exact, 1,
            "on_get_snapshot fixture values mismatch"
        );
    }

    if loaded_names.contains("add_vote") {
        let vote_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_votes
             WHERE receipt_id = '3QMkZ9sWqHaRkCT93kbqpMQPf49jwym6EAqYL2LxJ7pa'
               AND event_type = 'add_vote'
               AND proposal_id = 2
               AND account_id = 'lane.near'
               AND vote = 0
               AND account_balance = '32279877702596545200000002'",
        )
        .await?;
        assert_eq!(vote_exact, 1, "add_vote fixture values mismatch");

        let voting_power_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_account_voting_power_events
             WHERE receipt_id = '3QMkZ9sWqHaRkCT93kbqpMQPf49jwym6EAqYL2LxJ7pa'
               AND event_type = 'add_vote'
               AND proposal_id = 2
               AND account_id = 'lane.near'
               AND voting_power = '32279877702596545200000002'",
        )
        .await?;
        assert_eq!(
            voting_power_exact, 1,
            "add_vote voting_power fixture values mismatch"
        );
    }

    if loaded_names.contains("ft_mint") {
        let mint_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_account_balances
             WHERE receipt_id = 'ESiHP4QSHA6Rr3xsjKT8j7Qq8JYgnk92X9GmKyqm4HNr'
               AND event_type = 'ft_mint'
               AND account_id = 'jchos.near'
               AND delta_amount = '100000000000000000000000'",
        )
        .await?;
        assert_eq!(mint_exact, 1, "ft_mint fixture values mismatch");
    }

    if loaded_names.contains("ft_burn") {
        let burn_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_account_balances
             WHERE receipt_id = 'E9eZJN418Qnhc1zKUfHmL9X6SkkjLTePXXyXAyhZR62W'
               AND event_type = 'ft_burn'
               AND account_id = 'jchos.near'
               AND delta_amount = '-2100016428726328300000001'",
        )
        .await?;
        assert_eq!(burn_exact, 1, "ft_burn fixture values mismatch");

        let mint_side_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_account_balances
             WHERE receipt_id = 'E9eZJN418Qnhc1zKUfHmL9X6SkkjLTePXXyXAyhZR62W'
               AND event_type = 'ft_mint'
               AND account_id = 'athis.near'
               AND delta_amount = '2106360231141328300000001'",
        )
        .await?;
        assert_eq!(
            mint_side_exact, 1,
            "ft_burn fixture minted-side values mismatch"
        );
    }

    if loaded_names.contains("lockup_action") {
        let lockup_action_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_lockup_actions
             WHERE receipt_id = '293NB2ES5jYn8RPP8RTUKQeDc8mqZrr1d8EcGojpT3vL'
               AND lockup_account_id = '0887fe221cafb6810909c83393e23585c439fe99.venear.dao'
               AND owner_account_id = 'jchos.near'
               AND predecessor_account_id = 'venear.dao'
               AND receiver_account_id = '0887fe221cafb6810909c83393e23585c439fe99.venear.dao'
               AND method_name = 'new'
               AND action_source = 'receipt'
               AND attached_deposit = '2.00 NEAR'
               AND success = true
               AND args_json->>'owner_account_id' = 'jchos.near'
               AND args_json->>'min_lockup_deposit' = '2000000000000000000000000'",
        )
        .await?;
        assert_eq!(
            lockup_action_exact, 1,
            "lockup_action fixture values mismatch"
        );
    }

    if loaded_names.contains("begin_unlock_near") {
        let begin_unlock_event_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_lockup_events
             WHERE receipt_id = 'ESd1k51dQSGWZbRDXVMkX7S5A3T6ME8WoD1m9V9VeUas'
               AND event_type = 'begin_unlock_near'
               AND lockup_account_id = '8a8e68455f59ba24082ecd3007d8b9a33a21a651.venear.dao'
               AND predecessor_account_id = 'nguyentran14.near'
               AND contract_id = '8a8e68455f59ba24082ecd3007d8b9a33a21a651.venear.dao'
               AND success = true",
        )
        .await?;
        assert_eq!(
            begin_unlock_event_exact, 1,
            "begin_unlock_near event values mismatch"
        );

        let begin_unlock_action_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_lockup_actions
             WHERE receipt_id = 'ESd1k51dQSGWZbRDXVMkX7S5A3T6ME8WoD1m9V9VeUas'
               AND method_name = 'begin_unlock_near'
               AND lockup_account_id = '8a8e68455f59ba24082ecd3007d8b9a33a21a651.venear.dao'
               AND predecessor_account_id = 'nguyentran14.near'
               AND attached_deposit = '<0.001 NEAR'
               AND args_json->>'amount' = '4426104630583193338115205215'",
        )
        .await?;
        assert_eq!(
            begin_unlock_action_exact, 1,
            "begin_unlock_near action values mismatch"
        );
    }

    if loaded_names.contains("begin_unlock_near_horbarr_after_delegate") {
        let begin_unlock_horbarr_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_lockup_actions
             WHERE receipt_id = '2JwtSaiHbYJ2EQdKBZ9HEq9ietJSVMAR4hoeASZFcSsd'
               AND method_name = 'begin_unlock_near'
               AND lockup_account_id = 'b155f46a0ec06336370ab6a9b26607779df26b9d.venear.dao'
               AND predecessor_account_id = 'horbarr.near'
               AND attached_deposit = '<0.001 NEAR'
               AND args_json->>'amount' = '5000010933856189300000000'",
        )
        .await?;
        assert_eq!(
            begin_unlock_horbarr_exact, 1,
            "begin_unlock_near_horbarr_after_delegate values mismatch"
        );
    }

    if loaded_names.contains("end_unlock_near") {
        let end_unlock_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_lockup_actions
             WHERE receipt_id = '7go48KQnnZaRcAmZsoZiipsnkvwnhQxw7H7dzS7CzRJK'
               AND method_name = 'end_unlock_near'
               AND lockup_account_id = 'ae6364414a4e0357b3476ab1fb56e2e61992f030.venear.dao'
               AND predecessor_account_id = 'full_master_ok.near'
               AND attached_deposit = '<0.001 NEAR'
               AND args_json->>'amount' = '5400010747248653000000000'",
        )
        .await?;
        assert_eq!(
            end_unlock_exact, 1,
            "end_unlock_near fixture values mismatch"
        );
    }

    if loaded_names.contains("delegate_all") {
        let delegate_all_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_delegation_events
             WHERE receipt_id = '2RLxWawCY1LXJSR7dv6knY7CY63BQpzuYy6pW3xKth85'
               AND event_type = 'delegate_all'
               AND delegator_account_id = 'horbarr.near'
               AND delegatee_account_id = 'sabrinanear.near'
               AND action_source = 'transaction'
               AND success = true
               AND attached_deposit = '<0.001 NEAR'
               AND args_json->>'receiver_id' = 'sabrinanear.near'",
        )
        .await?;
        assert_eq!(
            delegate_all_exact, 1,
            "delegate_all fixture values mismatch"
        );
    }

    if loaded_names.contains("delegate_all_horbarr_to_sabrinanear") {
        let delegate_horbarr_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_delegation_events
             WHERE receipt_id = 'Dh1MPT5dGvAzfXfnEiw2QSgHxC6sFzo1BUevhuy2fZE7'
               AND event_type = 'delegate_all'
               AND delegator_account_id = 'horbarr.near'
               AND delegatee_account_id = 'sabrinanear.near'
               AND action_source = 'receipt'
               AND success = true",
        )
        .await?;
        assert_eq!(
            delegate_horbarr_exact, 1,
            "delegate_all_horbarr fixture delegation values mismatch"
        );

        let delegate_horbarr_burn = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_account_balances
             WHERE receipt_id = 'Dh1MPT5dGvAzfXfnEiw2QSgHxC6sFzo1BUevhuy2fZE7'
               AND event_type = 'ft_burn'
               AND account_id = 'horbarr.near'
               AND delta_amount = '-5100010940196189300000001'",
        )
        .await?;
        assert_eq!(
            delegate_horbarr_burn, 1,
            "delegate_all_horbarr fixture burn values mismatch"
        );
    }

    if loaded_names.contains("undelegate") {
        let undelegate_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_delegation_events
             WHERE receipt_id = 'J3FGyjWYbVmqAMrmvt5u1mC6a4YcBBCYwGvMmsHH41eU'
               AND event_type = 'undelegate'
               AND delegator_account_id = 'thomasvd.near'
               AND delegatee_account_id IS NULL
               AND action_source = 'transaction'
               AND success = true
               AND attached_deposit = '<0.001 NEAR'",
        )
        .await?;
        assert_eq!(undelegate_exact, 1, "undelegate fixture values mismatch");
    }

    if loaded_names.contains("delegate_all_failed") {
        let delegate_all_failed_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_delegation_events
             WHERE receipt_id = 'DK7EtPBCk8etAXgJ6nAUQNzP8K6hyCSAr5r7r7inTgeW'
               AND event_type = 'delegate_all'
               AND delegator_account_id = '1920804333ced7e8357b5b22b69b3ce03aff94c04836ba604019e0c363f2d7b3'
               AND delegatee_account_id = 'vote.mob.near'
               AND action_source = 'receipt'
               AND success = false
               AND args_json->>'receiver_id' = 'vote.mob.near'",
        )
        .await?;
        assert_eq!(
            delegate_all_failed_exact, 1,
            "delegate_all_failed fixture values mismatch"
        );
    }

    if loaded_names.contains("undelegate_failed") {
        let undelegate_failed_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_delegation_events
             WHERE receipt_id = '7FwhPWS451YFWHgyAhXjaXKiiRPXdi2Vp4CqMCoW3GiU'
               AND event_type = 'undelegate'
               AND delegator_account_id = 'lilopposum556.tg'
               AND delegatee_account_id IS NULL
               AND action_source = 'receipt'
               AND success = false",
        )
        .await?;
        assert_eq!(
            undelegate_failed_exact, 1,
            "undelegate_failed fixture values mismatch"
        );
    }

    if loaded_names.contains("on_lockup_deployed") {
        let deployed_horbarr_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_lockup_events
             WHERE receipt_id = '5kyoC3u2sitcyBKpB7UvGzH9fYY69Lgq5DTQYnDCGkfn'
               AND event_type = 'lockup_deployed'
               AND lockup_account_id = 'b155f46a0ec06336370ab6a9b26607779df26b9d.venear.dao'
               AND owner_account_id = 'horbarr.near'
               AND lockup_version = 1",
        )
        .await?;
        assert_eq!(
            deployed_horbarr_exact, 1,
            "on_lockup_deployed event values mismatch"
        );
    }

    if loaded_names.contains("on_lockup_update_delegatee_burn_horbarr") {
        let callback_horbarr_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_lockup_events
             WHERE receipt_id = 'FPNphxYAxh76SsPdTumDSQJfYbHEe5fWVHE2juvo551Z'
               AND event_type = 'lockup_update_callback'
               AND lockup_account_id = 'b155f46a0ec06336370ab6a9b26607779df26b9d.venear.dao'
               AND owner_account_id = 'horbarr.near'
               AND lockup_version = 1
               AND lockup_update_nonce IS NULL
               AND locked_near_balance IS NULL
               AND action_source = 'receipt'
               AND predecessor_account_id = 'b155f46a0ec06336370ab6a9b26607779df26b9d.venear.dao'
               AND contract_id = 'venear.dao'
               AND success = true
               AND payload->>'version' = '1'
               AND payload->'update'->'V1'->>'lockup_update_nonce' = '187483094000002'
               AND payload->'update'->'V1'->>'locked_near_balance' = '1'",
        )
        .await?;
        assert_eq!(
            callback_horbarr_exact, 1,
            "on_lockup_update_delegatee callback values mismatch"
        );

        let delegatee_burn_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_account_balances
             WHERE receipt_id = 'FPNphxYAxh76SsPdTumDSQJfYbHEe5fWVHE2juvo551Z'
               AND event_type = 'ft_burn'
               AND account_id = 'sabrinanear.near'
               AND delta_amount = '-5000016432221189300000000'",
        )
        .await?;
        assert_eq!(
            delegatee_burn_exact, 1,
            "on_lockup_update_delegatee burn values mismatch"
        );
    }

    if loaded_names.contains("admin_set_lockup_contract") {
        let admin_set_lockup_contract_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_admin_events
             WHERE receipt_id = 'C3GaYA41YLM3Fb218aM7vDjn7nVb1BVGQYXJZTY8FDqA'
               AND contract_id = 'venear.dao'
               AND method_name = 'set_lockup_contract'
               AND predecessor_account_id = 'hos-root.sputnik-dao.near'
               AND attached_deposit = '<0.001 NEAR'
               AND success = true
               AND args_json->>'contract_hash' = 'EV4eXNuKVkcYisktcT4sk9XfFFRvcefy51Qs2hQkhnK1'
               AND args_json->>'min_lockup_deposit' = '2000000000000000000000000'",
        )
        .await?;
        assert_eq!(
            admin_set_lockup_contract_exact, 1,
            "admin_set_lockup_contract fixture values mismatch"
        );
    }

    if loaded_names.contains("admin_set_lockup_code_deployers") {
        let admin_set_lockup_code_deployers_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_admin_events
             WHERE receipt_id = '5mgLns7fdsQS2PJWTfDXd4MpZ74ZuAk9EovRKWhpv9jN'
               AND contract_id = 'venear.dao'
               AND method_name = 'set_lockup_code_deployers'
               AND predecessor_account_id = 'grishko_loh.near'
               AND attached_deposit = '0 NEAR'
               AND success = true
               AND args_json->>'lockup_code_deployers' = 'grishko_loh.near'",
        )
        .await?;
        assert_eq!(
            admin_set_lockup_code_deployers_exact, 1,
            "admin_set_lockup_code_deployers fixture values mismatch"
        );
    }

    if loaded_names.contains("admin_prepare_lockup_code") {
        let admin_prepare_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_admin_events
             WHERE receipt_id = '7zXJm5GJsEfoKdnrkXavqRoBmUzLYrVWr35FCQvmhHD6'
               AND contract_id = 'venear.dao'
               AND method_name = 'prepare_lockup_code'
               AND predecessor_account_id = 'root.near'
               AND attached_deposit = '1.98 NEAR'
               AND success = true
               AND args_json IS NULL
               AND LENGTH(args_base64) > 1000",
        )
        .await?;
        assert_eq!(
            admin_prepare_exact, 1,
            "admin_prepare_lockup_code fixture values mismatch"
        );
    }

    if loaded_names.contains("admin_set_guardians") {
        let admin_set_guardians_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_admin_events
             WHERE receipt_id = 'BHSb4GwmvaeaRx96hKEnAg9ktXm6CpjCmNGyBiQEKVsB'
               AND contract_id = 'vote.dao'
               AND method_name = 'set_guardians'
               AND predecessor_account_id = 'hos-root.sputnik-dao.near'
               AND attached_deposit = '<0.001 NEAR'
               AND success = true
               AND json_array_length(args_json->'guardians') = 5",
        )
        .await?;
        assert_eq!(
            admin_set_guardians_exact, 1,
            "admin_set_guardians fixture values mismatch"
        );
    }

    if loaded_names.contains("admin_set_reviewer_ids") {
        let admin_set_reviewers_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_admin_events
             WHERE receipt_id = 'BHSb4GwmvaeaRx96hKEnAg9ktXm6CpjCmNGyBiQEKVsB'
               AND contract_id = 'vote.dao'
               AND method_name = 'set_reviewer_ids'
               AND predecessor_account_id = 'hos-root.sputnik-dao.near'
               AND attached_deposit = '<0.001 NEAR'
               AND success = true
               AND json_array_length(args_json->'reviewer_ids') = 7
               AND EXISTS (
                   SELECT 1
                   FROM json_array_elements_text(args_json->'reviewer_ids') AS reviewer(value)
                   WHERE reviewer.value = 'gauntletgov.near'
               )",
        )
        .await?;
        assert_eq!(
            admin_set_reviewers_exact, 1,
            "admin_set_reviewer_ids fixture values mismatch"
        );
    }

    if loaded_names.contains("admin_upgrade") {
        let admin_upgrade_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_admin_events
             WHERE receipt_id = '2dFPt3dsPescVt8N8eYhcY55WYUtJJNA5j9RsfGYehup'
               AND contract_id = 'vote.dao'
               AND method_name = 'upgrade'
               AND predecessor_account_id = 'hos-root.sputnik-dao.near'
               AND attached_deposit = '0 NEAR'
               AND success = true
               AND args_json IS NULL
               AND LENGTH(args_base64) > 1000",
        )
        .await?;
        assert_eq!(
            admin_upgrade_exact, 1,
            "admin_upgrade fixture values mismatch"
        );
    }

    if loaded_names.contains("admin_migrate_state") {
        let admin_migrate_exact = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_venear_admin_events
             WHERE receipt_id = 'HCgYD6jbpwr9kVsfbpZ2Mw45rp5xUWdL68ZceuAFUP4a'
               AND contract_id = 'vote.dao'
               AND method_name = 'migrate_state'
               AND predecessor_account_id = 'vote.dao'
               AND attached_deposit = '0 NEAR'
               AND success = true
               AND args_json::text = '{}'",
        )
        .await?;
        assert_eq!(
            admin_migrate_exact, 1,
            "admin_migrate_state fixture values mismatch"
        );
    }

    eprintln!(
        "Processed {} fixtures ({} distinct blocks) → {} raw event rows",
        fixtures.len(),
        distinct_blocks.len(),
        raw_count
    );

    Ok(())
}

/// on_lockup_deployed_failed: failed lockup deployment recorded in near_venear_lockup_events.
/// Uses a dedicated fixture with a failed on_lockup_deployed callback if available.
#[tokio::test]
async fn test_lockup_deployed_callback_failed() -> Result<()> {
    init_test_tracing();

    let Some(fixture) = try_load_fixture("on_lockup_deployed_failed") else {
        eprintln!("SKIP test_lockup_deployed_callback_failed: on_lockup_deployed_failed fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&fixture, conn.as_ref()).await?;

    // A failed deployment creates a lockup_deployed_callback entry but no lockup_deployed event.
    let callback_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE event_type = 'lockup_deployed_callback'",
    )
    .await?;
    assert!(
        callback_count >= 1,
        "expected >= 1 lockup_deployed_callback row even for failed deployments, got {callback_count}"
    );

    let deployed_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE event_type = 'lockup_deployed'",
    )
    .await?;
    assert_eq!(
        deployed_count, 0,
        "expected 0 lockup_deployed rows for a failed deployment block, got {deployed_count}"
    );

    Ok(())
}

/// begin_unlock_near → entry in near_venear_lockup_events and near_venear_lockup_actions.
#[tokio::test]
async fn test_begin_unlock_near() -> Result<()> {
    init_test_tracing();

    let Some(fixture) = try_load_fixture("begin_unlock_near") else {
        eprintln!("SKIP test_begin_unlock_near: begin_unlock_near fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&fixture, conn.as_ref()).await?;

    let event_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE event_type = 'begin_unlock_near'",
    )
    .await?;
    assert!(
        event_count >= 1,
        "expected >= 1 begin_unlock_near row in near_venear_lockup_events, got {event_count}"
    );

    let action_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_actions
         WHERE method_name = 'begin_unlock_near'",
    )
    .await?;
    assert!(
        action_count >= 1,
        "expected >= 1 begin_unlock_near row in near_venear_lockup_actions, got {action_count}"
    );

    Ok(())
}

/// end_unlock_near → entry in near_venear_lockup_events and near_venear_lockup_actions.
#[tokio::test]
async fn test_end_unlock_near() -> Result<()> {
    init_test_tracing();

    let Some(fixture) = try_load_fixture("end_unlock_near") else {
        eprintln!("SKIP test_end_unlock_near: end_unlock_near fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&fixture, conn.as_ref()).await?;

    let event_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE event_type = 'end_unlock_near'",
    )
    .await?;
    assert!(
        event_count >= 1,
        "expected >= 1 end_unlock_near row in near_venear_lockup_events, got {event_count}"
    );

    let action_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_actions
         WHERE method_name = 'end_unlock_near'",
    )
    .await?;
    assert!(
        action_count >= 1,
        "expected >= 1 end_unlock_near row in near_venear_lockup_actions, got {action_count}"
    );

    Ok(())
}

/// lock_pending_near → entry in near_venear_lockup_events and near_venear_lockup_actions.
#[tokio::test]
async fn test_lock_pending_near() -> Result<()> {
    init_test_tracing();

    let Some(fixture) = try_load_fixture("lock_pending_near") else {
        eprintln!("SKIP test_lock_pending_near: lock_pending_near fixture not found");
        return Ok(());
    };

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&fixture, conn.as_ref()).await?;

    let event_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_events
         WHERE event_type = 'lock_pending_near'",
    )
    .await?;
    assert!(
        event_count >= 1,
        "expected >= 1 lock_pending_near row in near_venear_lockup_events, got {event_count}"
    );

    let action_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockup_actions
         WHERE method_name = 'lock_pending_near'",
    )
    .await?;
    assert!(
        action_count >= 1,
        "expected >= 1 lock_pending_near row in near_venear_lockup_actions, got {action_count}"
    );

    Ok(())
}
