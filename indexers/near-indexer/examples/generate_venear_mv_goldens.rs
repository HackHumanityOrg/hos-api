//! Generate deterministic golden-row fixtures for HoS materialized views.
//!
//! Default output:
//! `tests/fixtures/venear_governance/expectations/hos_mv_golden_rows.json`
//!
//! Environment variables:
//! - `OVERWRITE_EXISTING=true|false` (default `false`)
//! - `OUTPUT_PATH=<custom path>`

#[path = "../tests/support/hos_mv_goldens.rs"]
#[allow(dead_code)]
mod hos_mv_goldens;

use anyhow::{bail, Context, Result};
use common::config::{
    AppConfig, DatabaseConfig, IndexerConfig, LoggingConfig, NearLakeConfig, NearModuleStreamMode,
    NearModulesConfig, NearStreamProvider, TelemetryConfig, VenearGovernanceModuleConfig,
};
use hos_mv_goldens::{capture_hos_mv_goldens, expected_fixture_path, write_hos_mv_golden_fixture};
use near_indexer::modules::venear_governance::VenearGovernanceModule;
use near_indexer::IndexerModule;
use near_lake_framework::near_indexer_primitives::StreamerMessage;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::path::{Path, PathBuf};
use test_support::TestDatabase;

const RANGE_SCENARIOS: &[&str] = &[
    "hos_lockup_core_167957",
    "hos_create_proposal_167957",
    "hos_approval_snapshot_168368",
    "hos_vote_168369",
    "hos_delegation_187483",
    "hos_config_185721",
];

fn parse_bool_env(name: &str, default: bool) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default)
}

fn load_fixture_range(name: &str) -> Vec<StreamerMessage> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/venear_governance/ranges")
        .join(format!("{name}.json"));

    let bytes = std::fs::read(&path)
        .unwrap_or_else(|err| panic!("failed to read fixture {}: {err}", path.display()));

    serde_json::from_slice(&bytes)
        .unwrap_or_else(|err| panic!("failed to deserialize fixture {}: {err}", path.display()))
}

fn load_all_fixture_messages() -> Vec<StreamerMessage> {
    let mut messages = Vec::new();

    for scenario in RANGE_SCENARIOS {
        let mut range = load_fixture_range(scenario);
        if range.is_empty() {
            panic!("fixture range should not be empty: {scenario}");
        }
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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,near_lake_framework=warn".into()),
        )
        .json()
        .init();

    let overwrite_existing = parse_bool_env("OVERWRITE_EXISTING", false);
    let output_path = std::env::var("OUTPUT_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| expected_fixture_path());

    if output_path.exists() && !overwrite_existing {
        bail!(
            "output already exists at {} (set OVERWRITE_EXISTING=true to replace)",
            output_path.display()
        );
    }

    let fixtures = load_all_fixture_messages();
    let db = TestDatabase::new().await;

    let module = VenearGovernanceModule::new(&build_venear_only_config());
    module.initialize(db.connection.as_ref()).await?;

    for fixture in &fixtures {
        module
            .process_block(fixture, db.connection.as_ref())
            .await
            .context("failed processing fixture block")?;
    }

    db.connection
        .execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT near.refresh_hos_materialized_views()",
            vec![],
        ))
        .await
        .context("failed to refresh HOS materialized views")?;

    let fixture = capture_hos_mv_goldens(db.connection.as_ref())
        .await
        .context("failed to capture HOS MV golden rows")?;

    write_hos_mv_golden_fixture(&output_path, &fixture)?;

    tracing::info!(
        path = %output_path.display(),
        case_count = fixture.cases.len(),
        "Generated HoS MV golden rows fixture"
    );

    Ok(())
}
