//! Generate deterministic per-fixture expectation artifacts for NEAR fixtures.
//!
//! Environment variables:
//! - `MODULE_KIND=raw|venear`
//! - `FIXTURE_ID=<fixture id>` or `ALL=true`
//! - `OVERWRITE_EXISTING=true|false` (default `false`)

#![allow(dead_code)]

#[path = "../tests/support/fixture_catalog.rs"]
mod fixture_catalog;
#[path = "../tests/support/fixture_expectations.rs"]
mod fixture_expectations;
#[path = "../tests/support/fixture_ranges.rs"]
mod fixture_ranges;

use anyhow::{bail, Context, Result};
use common::config::{
    AppConfig, DatabaseConfig, IndexerConfig, LoggingConfig, NearLakeConfig, NearModuleStreamMode,
    NearModulesConfig, NearStreamProvider, RawModuleConfig, TelemetryConfig,
    VenearGovernanceModuleConfig,
};
use fixture_catalog::{
    all_fixture_cases, find_fixture_case, load_fixture_messages, FixtureCase, ModuleExpectationKind,
};
use fixture_expectations::{
    capture_fixture_expectation, expectation_path, refresh_hos_materialized_views,
    reset_near_indexer_tables, write_fixture_expectation,
};
use near_indexer::modules::raw::RawModule;
use near_indexer::modules::venear_governance::VenearGovernanceModule;
use near_indexer::IndexerModule;
use test_support::TestDatabase;

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
        ..RawModuleConfig::default()
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

fn build_raw_only_config() -> AppConfig {
    build_test_app_config(Some(raw_config(true)), None)
}

fn build_venear_only_config() -> AppConfig {
    build_test_app_config(Some(raw_config(false)), Some(venear_test_config()))
}

fn selected_fixture_cases() -> Result<Vec<FixtureCase>> {
    if let Ok(fixture_id) = std::env::var("FIXTURE_ID") {
        return Ok(vec![find_fixture_case(&fixture_id)?]);
    }

    if parse_bool_env("ALL", false) {
        return all_fixture_cases();
    }

    bail!("set FIXTURE_ID=<fixture id> or ALL=true");
}

async fn generate_raw_expectation(db: &TestDatabase, fixture_case: &FixtureCase) -> Result<()> {
    let config = build_raw_only_config();
    let module = RawModule::new(&config);
    let fixtures = load_fixture_messages(fixture_case)?;

    module.initialize(db.connection.as_ref()).await?;
    for fixture in &fixtures {
        module
            .process_block(fixture, db.connection.as_ref())
            .await
            .with_context(|| format!("failed processing raw fixture {}", fixture_case.id))?;
    }

    Ok(())
}

async fn generate_venear_expectation(db: &TestDatabase, fixture_case: &FixtureCase) -> Result<()> {
    let config = build_venear_only_config();
    let module = VenearGovernanceModule::new(&config);
    let fixtures = load_fixture_messages(fixture_case)?;

    module.initialize(db.connection.as_ref()).await?;
    for fixture in &fixtures {
        module
            .process_block(fixture, db.connection.as_ref())
            .await
            .with_context(|| format!("failed processing venear fixture {}", fixture_case.id))?;
    }

    Ok(())
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

    let module_kind = ModuleExpectationKind::from_env(
        &std::env::var("MODULE_KIND")
            .context("MODULE_KIND is required and must be one of raw|venear")?,
    )?;
    let overwrite_existing = parse_bool_env("OVERWRITE_EXISTING", false);
    let fixture_cases = selected_fixture_cases()?;
    let db = TestDatabase::new().await;

    for fixture_case in fixture_cases {
        let output_path = expectation_path(&fixture_case, module_kind);
        if output_path.exists() && !overwrite_existing {
            bail!(
                "expectation already exists at {} (set OVERWRITE_EXISTING=true to replace)",
                output_path.display()
            );
        }

        reset_near_indexer_tables(db.connection.as_ref()).await?;
        match module_kind {
            ModuleExpectationKind::Raw => generate_raw_expectation(&db, &fixture_case).await?,
            ModuleExpectationKind::Venear => {
                generate_venear_expectation(&db, &fixture_case).await?
            }
        }

        refresh_hos_materialized_views(db.connection.as_ref()).await?;
        let expectation =
            capture_fixture_expectation(db.connection.as_ref(), &fixture_case, module_kind).await?;
        write_fixture_expectation(&output_path, &expectation)?;

        tracing::info!(
            fixture_id = %fixture_case.id,
            module_kind = module_kind.as_str(),
            path = %output_path.display(),
            "generated fixture expectation"
        );
    }

    Ok(())
}
