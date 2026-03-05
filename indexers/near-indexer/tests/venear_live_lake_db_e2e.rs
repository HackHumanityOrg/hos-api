#![cfg(feature = "live_rpc_tests")]

mod support;

use anyhow::{Context, Result};
use common::config::{
    AppConfig, DatabaseConfig, IndexerConfig, LoggingConfig, NearLakeConfig, NearModuleStreamMode,
    NearModulesConfig, NearStreamProvider, TelemetryConfig, VenearGovernanceModuleConfig,
};
use near_indexer::core::lake_stream::{create_lake_stream, LakeStreamConfig};
use near_indexer::modules::venear_governance::VenearGovernanceModule;
use near_indexer::IndexerModule;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::time::Duration;
use support::{init_test_tracing, TestDatabase};
use tokio_stream::{wrappers::ReceiverStream, StreamExt};

#[tokio::test]
#[ignore = "hits real NEAR Lake and requires NEAR_LIVE_TEST_START_HEIGHT"]
async fn venear_governance_live_lake_ingests_into_postgres() -> Result<()> {
    init_test_tracing();

    let start_height = match std::env::var("NEAR_LIVE_TEST_START_HEIGHT") {
        Ok(value) => value
            .parse::<u64>()
            .context("failed to parse NEAR_LIVE_TEST_START_HEIGHT as u64")?,
        Err(_) => {
            eprintln!("Skipping live lake test: set NEAR_LIVE_TEST_START_HEIGHT to run this test");
            return Ok(());
        }
    };

    let max_blocks = std::env::var("NEAR_LIVE_TEST_BLOCKS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(25);

    let db = TestDatabase::new().await;
    let conn = db.db();

    let module = VenearGovernanceModule::new(&build_live_test_config(start_height));
    module.initialize(conn.as_ref()).await?;

    let (_handle, stream) = create_lake_stream(LakeStreamConfig {
        chain: "mainnet".to_string(),
        start_height,
        concurrency: 1,
        preload_pool_size: 1,
    })?;

    let mut stream = ReceiverStream::new(stream);
    let mut processed = 0u64;

    while processed < max_blocks {
        let next_msg = tokio::time::timeout(Duration::from_secs(90), stream.next())
            .await
            .context("timed out waiting for next NEAR Lake block")?;

        let Some(message) = next_msg else {
            break;
        };

        module.process_block(&message, conn.as_ref()).await?;
        module
            .update_cursor(
                conn.as_ref(),
                message.block.header.height as i64,
                message.block.header.prev_height.map(|value| value as i64),
            )
            .await?;

        processed += 1;
    }

    assert!(processed > 0, "expected to process at least one block");

    let tracked_relevant_receipts = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.relevant_receipts
         WHERE scope = 'hos'",
    )
    .await?;
    let relevant_actions = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_receipt_actions ra
         WHERE EXISTS (
             SELECT 1
             FROM near.relevant_receipts rr
             WHERE rr.scope = 'hos'
               AND rr.receipt_id = ra.receipt_id
         )",
    )
    .await?;
    let relevant_outcomes = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_execution_outcomes eo
         WHERE EXISTS (
             SELECT 1
             FROM near.relevant_receipts rr
             WHERE rr.scope = 'hos'
               AND rr.receipt_id = eo.receipt_id
         )",
    )
    .await?;

    assert!(
        tracked_relevant_receipts > 0 && relevant_actions > 0 && relevant_outcomes > 0,
        "expected HoS-related canonical raw rows in sampled live window; increase NEAR_LIVE_TEST_BLOCKS or adjust start height"
    );

    Ok(())
}

fn build_live_test_config(start_height: u64) -> AppConfig {
    let mut db = DatabaseConfig::default();
    db.url = "postgres://unused".to_string();

    let cfg = IndexerConfig {
        database: db,
        logging: LoggingConfig::default(),
        telemetry: TelemetryConfig::default(),
        discourse: None,
        telegram: None,
        near: Some(NearLakeConfig {
            provider: NearStreamProvider::Lake,
            chain: "mainnet".to_string(),
            start_height,
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
                    mview_refresh_interval_blocks: 100,
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
        .context("expected one row from scalar query")?;
    Ok(row.try_get("", "value")?)
}
