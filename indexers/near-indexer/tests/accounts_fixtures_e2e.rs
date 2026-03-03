//! Integration tests for the accounts module using real fixture messages.

mod support;

use anyhow::Result;
use common::config::{
    AccountsModuleConfig, AppConfig, DatabaseConfig, IndexerConfig, LoggingConfig, NearLakeConfig,
    NearModuleStreamMode, NearModulesConfig, NearStreamProvider, TelemetryConfig,
};
use near_indexer::modules::accounts::AccountsModule;
use near_indexer::IndexerModule;
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::path::Path;
use support::{init_test_tracing, TestDatabase};

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
                raw: None,
                accounts: Some(AccountsModuleConfig {
                    enabled: true,
                    stream_mode: NearModuleStreamMode::Shared,
                    start_height: None,
                }),
                tokens: None,
                staking_pools: None,
                venear_governance: None,
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

async fn scalar_string(db: &impl ConnectionTrait, sql: &str) -> Result<String> {
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

#[tokio::test]
async fn accounts_events_and_balances_are_replay_idempotent() -> Result<()> {
    init_test_tracing();

    let fixture = load_fixture("admin_set_guardians");
    let fixture_height = fixture.block.header.height as i64;
    let db = TestDatabase::new().await;
    let module = AccountsModule::new(&build_test_config());

    module.initialize(db.connection.as_ref()).await?;
    module
        .process_block(&fixture, db.connection.as_ref())
        .await?;

    let first_seen_count = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_account_events WHERE event_type = 'first_seen'",
    )
    .await?;
    assert!(first_seen_count > 0);

    let off_height_balance_rows = scalar_i64(
        db.connection.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_account_balances
             WHERE block_height <> {fixture_height}"
        ),
    )
    .await?;
    assert_eq!(
        off_height_balance_rows, 0,
        "all near_account_balances rows should come from fixture height"
    );

    let off_height_event_rows = scalar_i64(
        db.connection.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_account_events
             WHERE block_height <> {fixture_height}"
        ),
    )
    .await?;
    assert_eq!(
        off_height_event_rows, 0,
        "all near_account_events rows should come from fixture height"
    );

    let off_height_access_key_rows = scalar_i64(
        db.connection.as_ref(),
        &format!(
            "SELECT COUNT(*)::bigint AS value
             FROM near.near_access_keys
             WHERE last_seen_height <> {fixture_height}"
        ),
    )
    .await?;
    assert_eq!(
        off_height_access_key_rows, 0,
        "all near_access_keys rows should come from fixture height"
    );

    let vote_account_snapshot = scalar_string(
        db.connection.as_ref(),
        &format!(
            "SELECT account_type || '|' || parent_account_id || '|' || created_at_height::text || '|' || last_seen_height::text AS value
             FROM near.near_accounts
             WHERE account_id = 'vote.dao'"
        ),
    )
    .await?;
    assert_eq!(
        vote_account_snapshot,
        format!("named|dao|{fixture_height}|{fixture_height}")
    );

    let sample_balance_before = scalar_string(
        db.connection.as_ref(),
        "SELECT account_id || '|' || block_height::text || '|' || amount || '|' || locked || '|' || code_hash AS value
         FROM near.near_account_balances
         ORDER BY account_id ASC
         LIMIT 1",
    )
    .await?;
    let sample_event_before = scalar_string(
        db.connection.as_ref(),
        "SELECT account_id || '|' || event_type || '|' || block_height::text || '|' || shard_id::text || '|' || state_change_index::text AS value
         FROM near.near_account_events
         ORDER BY account_id ASC, event_index ASC
         LIMIT 1",
    )
    .await?;
    let sample_key_before = scalar_string(
        db.connection.as_ref(),
        "SELECT account_id || '|' || public_key || '|' || nonce::text || '|' || last_seen_height::text || '|' ||
                CASE WHEN is_active THEN '1' ELSE '0' END AS value
         FROM near.near_access_keys
         ORDER BY account_id ASC, public_key ASC
         LIMIT 1",
    )
    .await?;

    let balances_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_account_balances",
    )
    .await?;
    let events_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_account_events",
    )
    .await?;
    let access_keys_before = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_access_keys",
    )
    .await?;

    // Replay the same block message.
    module
        .process_block(&fixture, db.connection.as_ref())
        .await?;

    let balances_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_account_balances",
    )
    .await?;
    let events_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_account_events",
    )
    .await?;
    let access_keys_after = scalar_i64(
        db.connection.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.near_access_keys",
    )
    .await?;

    assert_eq!(balances_before, balances_after);
    assert_eq!(events_before, events_after);
    assert_eq!(access_keys_before, access_keys_after);

    let sample_balance_after = scalar_string(
        db.connection.as_ref(),
        "SELECT account_id || '|' || block_height::text || '|' || amount || '|' || locked || '|' || code_hash AS value
         FROM near.near_account_balances
         ORDER BY account_id ASC
         LIMIT 1",
    )
    .await?;
    let sample_event_after = scalar_string(
        db.connection.as_ref(),
        "SELECT account_id || '|' || event_type || '|' || block_height::text || '|' || shard_id::text || '|' || state_change_index::text AS value
         FROM near.near_account_events
         ORDER BY account_id ASC, event_index ASC
         LIMIT 1",
    )
    .await?;
    let sample_key_after = scalar_string(
        db.connection.as_ref(),
        "SELECT account_id || '|' || public_key || '|' || nonce::text || '|' || last_seen_height::text || '|' ||
                CASE WHEN is_active THEN '1' ELSE '0' END AS value
         FROM near.near_access_keys
         ORDER BY account_id ASC, public_key ASC
         LIMIT 1",
    )
    .await?;

    assert_eq!(sample_balance_before, sample_balance_after);
    assert_eq!(sample_event_before, sample_event_after);
    assert_eq!(sample_key_before, sample_key_after);

    Ok(())
}

#[tokio::test]
async fn accounts_snapshot_uses_first_seen_height() -> Result<()> {
    init_test_tracing();

    let fixture = load_fixture("admin_migrate_state");
    let fixture_height = fixture.block.header.height as i64;

    let db = TestDatabase::new().await;
    let module = AccountsModule::new(&build_test_config());

    module.initialize(db.connection.as_ref()).await?;
    module
        .process_block(&fixture, db.connection.as_ref())
        .await?;

    let row = db
        .connection
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT created_at_height::bigint AS value FROM near.near_accounts WHERE account_id = $1",
            vec!["vote.dao".into()],
        ))
        .await?
        .expect("expected vote.dao row");
    let created_at_height: i64 = row.try_get("", "value")?;

    assert_eq!(created_at_height, fixture_height);

    let vote_account_fields = scalar_string(
        db.connection.as_ref(),
        "SELECT account_type || '|' || parent_account_id || '|' ||
                CASE WHEN is_contract THEN '1' ELSE '0' END || '|' ||
                last_seen_height::text AS value
         FROM near.near_accounts
         WHERE account_id = 'vote.dao'",
    )
    .await?;
    assert_eq!(
        vote_account_fields,
        format!("named|dao|1|{fixture_height}"),
        "vote.dao snapshot fields should be populated deterministically"
    );

    Ok(())
}
