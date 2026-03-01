//! Integration tests for the veNEAR governance module using fixture files.
//!
//! Each test loads one or more pre-recorded `StreamerMessage` JSON files from
//! `tests/fixtures/venear_governance/`, processes them through
//! `VenearGovernanceModule` backed by a real Postgres instance (testcontainers),
//! and asserts correct DB state across ALL relevant tables.
//!
//! Fixtures are generated once by `examples/fetch_venear_fixtures.rs` and then
//! committed to the repository. Most tests require real fixtures; for event
//! types that are rare or currently unavailable in committed fixtures, tests
//! use a minimal synthetic fixture derived from a related real block.

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
use serde_json::json;
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

fn synthetic_proposal_reject_fixture_from_create(
    create_fixture: &StreamerMessage,
) -> StreamerMessage {
    let mut value = serde_json::to_value(create_fixture)
        .expect("failed to convert create fixture into mutable json");

    let shards = value
        .get_mut("shards")
        .and_then(|v| v.as_array_mut())
        .expect("fixture missing shards array");

    for shard in shards {
        let Some(receipt_outcomes) = shard
            .get_mut("receipt_execution_outcomes")
            .and_then(|v| v.as_array_mut())
        else {
            continue;
        };

        for receipt_outcome in receipt_outcomes {
            let Some(logs) = receipt_outcome
                .get_mut("execution_outcome")
                .and_then(|v| v.get_mut("outcome"))
                .and_then(|v| v.get_mut("logs"))
                .and_then(|v| v.as_array_mut())
            else {
                continue;
            };

            for log in logs {
                let Some(log_text) = log.as_str() else {
                    continue;
                };
                if !log_text.starts_with("EVENT_JSON:") {
                    continue;
                }

                let Ok(mut payload) = serde_json::from_str::<serde_json::Value>(&log_text[11..])
                else {
                    continue;
                };

                if payload.get("standard").and_then(|v| v.as_str()) != Some("venear")
                    || payload.get("event").and_then(|v| v.as_str()) != Some("create_proposal")
                {
                    continue;
                }

                let Some(first_data) = payload
                    .get("data")
                    .and_then(|v| v.as_array())
                    .and_then(|items| items.first())
                else {
                    continue;
                };

                let proposal_id = first_data
                    .get("proposal_id")
                    .cloned()
                    .unwrap_or_else(|| json!(0));
                let account_id = first_data
                    .get("proposer_id")
                    .cloned()
                    .unwrap_or_else(|| json!("unknown.near"));

                payload["event"] = json!("proposal_reject");
                payload["data"] = json!([{
                    "account_id": account_id,
                    "proposal_id": proposal_id,
                    "voting_start_time_sec": null
                }]);

                *log = json!(format!(
                    "EVENT_JSON:{}",
                    serde_json::to_string(&payload)
                        .expect("failed to serialize synthetic reject payload")
                ));

                return serde_json::from_value(value)
                    .expect("failed to deserialize synthetic proposal_reject fixture");
            }
        }
    }

    panic!("Failed to synthesize proposal_reject fixture from create_proposal fixture")
}

fn synthetic_lockup_delete_fixture_from_deployed(
    deployed_fixture: &StreamerMessage,
) -> StreamerMessage {
    let mut value = serde_json::to_value(deployed_fixture)
        .expect("failed to convert deployed fixture into mutable json");

    let shards = value
        .get_mut("shards")
        .and_then(|v| v.as_array_mut())
        .expect("fixture missing shards array");

    for shard in shards {
        let Some(receipt_outcomes) = shard
            .get_mut("receipt_execution_outcomes")
            .and_then(|v| v.as_array_mut())
        else {
            continue;
        };

        for receipt_outcome in receipt_outcomes {
            let Some(logs) = receipt_outcome
                .get_mut("execution_outcome")
                .and_then(|v| v.get_mut("outcome"))
                .and_then(|v| v.get_mut("logs"))
                .and_then(|v| v.as_array_mut())
            else {
                continue;
            };

            for log in logs {
                let Some(log_text) = log.as_str() else {
                    continue;
                };
                if !log_text.starts_with("EVENT_JSON:") {
                    continue;
                }

                let Ok(mut payload) = serde_json::from_str::<serde_json::Value>(&log_text[11..])
                else {
                    continue;
                };

                if payload.get("standard").and_then(|v| v.as_str()) != Some("venear")
                    || payload.get("event").and_then(|v| v.as_str()) != Some("lockup_deployed")
                {
                    continue;
                }

                payload["event"] = json!("lockup_delete");

                *log = json!(format!(
                    "EVENT_JSON:{}",
                    serde_json::to_string(&payload)
                        .expect("failed to serialize synthetic lockup_delete payload")
                ));

                return serde_json::from_value(value)
                    .expect("failed to deserialize synthetic lockup_delete fixture");
            }
        }
    }

    panic!("Failed to synthesize lockup_delete fixture from lockup_deployed fixture")
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

/// Lockup deployed → active entry in near_venear_lockups; raw event recorded.
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

    // near_venear_lockups: active lockup with correct suffix and discovery source
    let lockup_active = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockups WHERE is_active = true",
    )
    .await?;
    assert!(
        lockup_active >= 1,
        "expected >= 1 active lockup row after lockup_deployed, got {lockup_active}"
    );

    let lockup_deployed_source = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockups
         WHERE discovery_source = 'lockup_deployed_event'",
    )
    .await?;
    assert!(
        lockup_deployed_source >= 1,
        "expected lockup discovery_source = 'lockup_deployed_event', got {lockup_deployed_source}"
    );

    let lockup_suffix = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockups
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
/// so near_venear_lockups is also expected to have a row.
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

    // near_venear_lockups: on_lockup_update callback in the same block upserts a lockup
    let lockup_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockups",
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

/// Lockup delete → lockup marked inactive in near_venear_lockups.
#[tokio::test]
async fn test_lockup_delete() -> Result<()> {
    init_test_tracing();

    let deployed = load_fixture("lockup_deployed");
    let deleted = try_load_fixture("lockup_delete")
        .unwrap_or_else(|| synthetic_lockup_delete_fixture_from_deployed(&deployed));

    let db = TestDatabase::new().await;
    let conn = db.db();
    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    module.process_block(&deployed, conn.as_ref()).await?;
    module.process_block(&deleted, conn.as_ref()).await?;

    // near_venear_lockups: at least one inactive lockup with deleted_at_height set
    let inactive_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockups WHERE is_active = false",
    )
    .await?;
    assert!(
        inactive_count >= 1,
        "expected >= 1 inactive lockup row after lockup_delete, got {inactive_count}"
    );

    let deleted_at_set = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockups
         WHERE is_active = false AND deleted_at_height IS NOT NULL",
    )
    .await?;
    assert!(
        deleted_at_set >= 1,
        "expected deleted lockup to have deleted_at_height set, got {deleted_at_set}"
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

    let create = load_fixture("create_proposal");
    let reject = try_load_fixture("proposal_reject")
        .unwrap_or_else(|| synthetic_proposal_reject_fixture_from_create(&create));

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
    }

    Ok(())
}

/// on_lockup_update callback → lockup upserted in near_venear_lockups.
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

    // near_venear_lockups: lockup upserted with correct suffix (from callback)
    let lockup_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockups",
    )
    .await?;
    assert!(
        lockup_count >= 1,
        "expected >= 1 lockup row after on_lockup_update callback, got {lockup_count}"
    );

    let lockup_active = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockups
         WHERE is_active = true AND lockup_account_id LIKE '%.venear.dao'",
    )
    .await?;
    assert!(
        lockup_active >= 1,
        "expected >= 1 active .venear.dao lockup after on_lockup_update, got {lockup_active}"
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

    // near_venear_lockups: lockup_deployed pre-load created a row
    let lockup_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockups WHERE is_active = true",
    )
    .await?;
    assert!(
        lockup_count >= 1,
        "expected >= 1 active lockup row (from lockup_deployed pre-load), got {lockup_count}"
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
        "create_proposal",
        "proposal_approve",
        "proposal_reject",
        "on_get_snapshot",
        "add_vote",
        "remove_vote",
        "ft_mint",
        "ft_burn",
        "on_lockup_update",
        "lockup_action",
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

    // near_venear_events_raw: at least one row per distinct block
    let raw_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw",
    )
    .await?;

    let distinct_blocks: std::collections::HashSet<u64> = fixtures
        .iter()
        .map(|(_, f)| f.block.header.height)
        .collect();

    assert!(
        raw_count >= distinct_blocks.len() as i64,
        "expected at least {} raw event rows (one per distinct fixture block), got {}",
        distinct_blocks.len(),
        raw_count
    );

    // near_venear_lockups: populated by lockup_deployed and/or on_lockup_update
    if loaded_names.contains("lockup_deployed") || loaded_names.contains("on_lockup_update") {
        let lockup_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockups",
        )
        .await?;
        assert!(
            lockup_count >= 1,
            "expected >= 1 row in near_venear_lockups after loading lockup fixtures, got {lockup_count}"
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

    eprintln!(
        "Processed {} fixtures ({} distinct blocks) → {} raw event rows",
        fixtures.len(),
        distinct_blocks.len(),
        raw_count
    );

    Ok(())
}
