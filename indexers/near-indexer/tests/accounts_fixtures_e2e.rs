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

#[tokio::test]
async fn accounts_events_and_balances_are_replay_idempotent() -> Result<()> {
    init_test_tracing();

    let fixture = load_fixture("admin_set_guardians");
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

    Ok(())
}
