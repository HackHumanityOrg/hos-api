use anyhow::{Context, Result};
use common::config::{AppConfig, NearModuleStreamMode, NearStreamProvider, ToLog};
use common::telemetry::{self, TelemetrySetup};
use db_core::connection::from_config as db_from_config;
use futures::future::try_join_all;
use near_indexer::{modules, ModuleRegistry};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use tracing::{error, info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
struct StreamAssignment {
    stream_id: String,
    module_names: Vec<String>,
}

fn plan_stream_assignments(registry: &ModuleRegistry) -> Vec<StreamAssignment> {
    let modules = registry
        .enabled_modules()
        .map(|module| (module.name().to_string(), module.stream_mode()))
        .collect::<Vec<_>>();

    plan_stream_assignments_from_modes(&modules)
}

fn plan_stream_assignments_from_modes(
    modules: &[(String, NearModuleStreamMode)],
) -> Vec<StreamAssignment> {
    let mut shared_modules = Vec::new();
    let mut independent_assignments = Vec::new();

    for (module_name, stream_mode) in modules {
        match stream_mode {
            NearModuleStreamMode::Shared => shared_modules.push(module_name.clone()),
            NearModuleStreamMode::Independent => {
                independent_assignments.push(StreamAssignment {
                    stream_id: format!("module:{module_name}"),
                    module_names: vec![module_name.clone()],
                });
            }
        }
    }

    let mut assignments = Vec::new();
    if !shared_modules.is_empty() {
        assignments.push(StreamAssignment {
            stream_id: "shared".to_string(),
            module_names: shared_modules,
        });
    }
    assignments.extend(independent_assignments);

    assignments
}

fn configured_module_start_heights(config: &AppConfig) -> HashMap<String, u64> {
    let mut start_heights = HashMap::new();

    let AppConfig::Indexer(idx) = config else {
        return start_heights;
    };

    let Some(near) = idx.near.as_ref() else {
        return start_heights;
    };

    let Some(modules) = near.modules.as_ref() else {
        return start_heights;
    };

    if let Some(start_height) = modules.raw.as_ref().and_then(|m| m.start_height) {
        start_heights.insert("raw".to_string(), start_height);
    }
    if let Some(start_height) = modules.accounts.as_ref().and_then(|m| m.start_height) {
        start_heights.insert("accounts".to_string(), start_height);
    }
    if let Some(start_height) = modules.tokens.as_ref().and_then(|m| m.start_height) {
        start_heights.insert("tokens".to_string(), start_height);
    }
    if let Some(start_height) = modules.staking_pools.as_ref().and_then(|m| m.start_height) {
        start_heights.insert("staking_pools".to_string(), start_height);
    }
    if let Some(start_height) = modules
        .venear_governance
        .as_ref()
        .and_then(|m| m.start_height)
    {
        start_heights.insert("venear_governance".to_string(), start_height);
    }

    start_heights
}

async fn resolve_stream_start_height(
    db: &sea_orm::DatabaseConnection,
    stream_id: &str,
    module_names: &[String],
    configured_start_height: u64,
    module_start_heights: &HashMap<String, u64>,
) -> Result<(u64, bool)> {
    let mut stream_start_height: Option<u64> = None;
    let mut any_module_has_cursor = false;

    for module_name in module_names {
        let module_explicit_start_height = module_start_heights.get(module_name).copied();
        let cursor = near_indexer::core::cursor::get_module_cursor(db, module_name).await?;

        let module_start_height = if let Some(height) = cursor {
            let next_height = height.saturating_add(1).max(0) as u64;
            any_module_has_cursor = true;

            info!(
                stream_id = stream_id,
                module = module_name,
                cursor = height,
                next_height,
                "Module cursor found"
            );

            next_height
        } else {
            let module_start_height =
                module_explicit_start_height.unwrap_or(configured_start_height);

            if module_start_height > 0 {
                // Seed in-memory cursor so out-of-order block completion still advances
                // contiguous cursors correctly from the configured start.
                let seed_height =
                    i64::try_from(module_start_height.saturating_sub(1)).unwrap_or(i64::MAX);
                near_indexer::core::cursor::seed_module_cursor(module_name, seed_height);
            }

            if module_explicit_start_height.is_some() {
                info!(
                    stream_id = stream_id,
                    module = module_name,
                    configured_start_height = module_start_height,
                    "No cursor found, will start from module-specific configured height"
                );
            } else {
                info!(
                    stream_id = stream_id,
                    module = module_name,
                    configured_start_height = module_start_height,
                    "No cursor found, will start from global configured height"
                );
            }

            module_start_height
        };

        stream_start_height = Some(
            stream_start_height
                .map(|current| current.min(module_start_height))
                .unwrap_or(module_start_height),
        );
    }

    Ok((
        stream_start_height.unwrap_or(configured_start_height),
        any_module_has_cursor,
    ))
}

#[allow(clippy::too_many_arguments)]
async fn run_stream_worker(
    registry: Arc<ModuleRegistry>,
    db: Arc<sea_orm::DatabaseConnection>,
    chain: String,
    stream_provider: NearStreamProvider,
    fastnear_api_key: Option<String>,
    fastnear_num_threads: Option<u64>,
    configured_start_height: u64,
    module_start_heights: Arc<HashMap<String, u64>>,
    configured_concurrency: usize,
    configured_preload_pool_size: usize,
    assignment: StreamAssignment,
) -> Result<()> {
    let assigned_modules = Arc::new(
        assignment
            .module_names
            .iter()
            .cloned()
            .collect::<HashSet<_>>(),
    );

    let mut restart_backoff = Duration::from_secs(1);
    let max_restart_backoff = Duration::from_secs(30);

    loop {
        let (stream_start_height, any_module_has_cursor) = match resolve_stream_start_height(
            db.as_ref(),
            &assignment.stream_id,
            &assignment.module_names,
            configured_start_height,
            module_start_heights.as_ref(),
        )
        .await
        {
            Ok(values) => values,
            Err(err) => {
                error!(
                    stream_id = %assignment.stream_id,
                    error = %err,
                    backoff_ms = restart_backoff.as_millis(),
                    "Failed resolving stream start, retrying worker"
                );

                tokio::time::sleep(restart_backoff).await;
                restart_backoff = (restart_backoff.saturating_mul(2)).min(max_restart_backoff);
                continue;
            }
        };

        info!(
            stream_id = %assignment.stream_id,
            assigned_modules = ?assignment.module_names,
            stream_provider = stream_provider.as_str(),
            stream_start_height = stream_start_height,
            resume_source = %if any_module_has_cursor { "cursor" } else { "config" },
            configured_concurrency = configured_concurrency,
            configured_preload_pool_size = configured_preload_pool_size,
            "Starting stream worker"
        );

        let lake_config = near_indexer::core::lake_stream::LakeStreamConfig {
            chain: chain.clone(),
            start_height: stream_start_height,
            concurrency: 1,
            preload_pool_size: configured_preload_pool_size,
        };

        let stream_parts = match stream_provider {
            NearStreamProvider::Lake => {
                near_indexer::core::lake_stream::create_lake_stream(lake_config)
            }
            NearStreamProvider::Fastnear => {
                near_indexer::core::lake_stream::create_fastnear_stream(
                    lake_config,
                    fastnear_api_key.clone(),
                    fastnear_num_threads,
                )
            }
        };

        let (streamer_handle, stream) = match stream_parts {
            Ok(stream_parts) => stream_parts,
            Err(err) => {
                error!(
                    stream_id = %assignment.stream_id,
                    stream_provider = stream_provider.as_str(),
                    error = %err,
                    backoff_ms = restart_backoff.as_millis(),
                    "Failed to create stream, retrying worker"
                );

                tokio::time::sleep(restart_backoff).await;
                restart_backoff = (restart_backoff.saturating_mul(2)).min(max_restart_backoff);
                continue;
            }
        };

        let mut streamer_handle = streamer_handle;
        restart_backoff = Duration::from_secs(1);
        let mut processing_task =
            tokio::spawn(near_indexer::core::processor::run_parallel_processing(
                registry.clone(),
                db.clone(),
                stream,
                configured_concurrency,
                assigned_modules.clone(),
                assignment.stream_id.clone(),
            ));

        tokio::select! {
            processing_result = &mut processing_task => {
                if !streamer_handle.is_finished() {
                    streamer_handle.abort();
                }

                match processing_result {
                    Ok(Ok(())) => warn!(
                        stream_id = %assignment.stream_id,
                        "Processing task exited unexpectedly, restarting worker"
                    ),
                    Ok(Err(err)) => error!(
                        stream_id = %assignment.stream_id,
                        error = %err,
                        "Processing task failed, restarting worker"
                    ),
                    Err(join_err) => error!(
                        stream_id = %assignment.stream_id,
                        error = %join_err,
                        "Processing task panicked, restarting worker"
                    ),
                }
            }
            streamer_result = &mut streamer_handle => {
                if !processing_task.is_finished() {
                    processing_task.abort();
                }

                match streamer_result {
                    Ok(Ok(())) => warn!(
                        stream_id = %assignment.stream_id,
                        stream_provider = stream_provider.as_str(),
                        "Streamer exited unexpectedly, restarting worker"
                    ),
                    Ok(Err(err)) => error!(
                        stream_id = %assignment.stream_id,
                        stream_provider = stream_provider.as_str(),
                        error = %err,
                        "Streamer failed, restarting worker"
                    ),
                    Err(join_err) => error!(
                        stream_id = %assignment.stream_id,
                        stream_provider = stream_provider.as_str(),
                        error = %join_err,
                        "Streamer panicked, restarting worker"
                    ),
                }
            }
        }

        tokio::time::sleep(restart_backoff).await;
        restart_backoff = (restart_backoff.saturating_mul(2)).min(max_restart_backoff);
    }
}

fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let config =
        AppConfig::load_indexer("near-indexer").context("Failed to load indexer config")?;

    let telemetry_setup =
        TelemetrySetup::from_config("near-indexer", config.telemetry(), &config.logging().level)?;
    let _guard = telemetry::init_telemetry(telemetry_setup)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Failed to create Tokio runtime")?;

    if let Err(err) = runtime.block_on(run(config)) {
        error!(error = ?err, "NEAR indexer terminated with error");
        return Err(err);
    }

    Ok(())
}

async fn run(config: AppConfig) -> Result<()> {
    let startup_time = Instant::now();

    info!(
        config = %config.to_log(),
        version = env!("CARGO_PKG_VERSION"),
        "Starting NEAR indexer"
    );

    let db = Arc::new(
        db_from_config(config.database())
            .await
            .context("DB connection failed")?,
    );

    // Initialize module registry
    let mut registry = ModuleRegistry::new();
    modules::register_all_modules(&mut registry, &config);

    // Initialize all modules
    registry.initialize_all(db.as_ref()).await?;

    // Initialize cursor storage from database
    near_indexer::core::cursor::initialize_cursor_storage(db.as_ref()).await?;
    info!("Cursor storage initialized from database");

    let registry = Arc::new(registry);

    // Check if any modules are enabled
    let enabled_count: usize = registry.enabled_modules().count();
    let enabled_module_names: Vec<&str> = registry.enabled_modules().map(|m| m.name()).collect();

    if enabled_count == 0 {
        warn!("No modules enabled, exiting gracefully");
        return Ok(());
    }

    info!(
        enabled_count = enabled_count,
        enabled_modules = ?enabled_module_names,
        "Modules initialized and ready"
    );

    let (
        chain,
        configured_provider,
        configured_fastnear_api_key,
        configured_fastnear_num_threads,
        configured_start_height,
        raw_concurrency,
        raw_preload_pool_size,
    ) = match &config {
        AppConfig::Indexer(idx) => {
            let idx = idx.as_ref();
            let n = idx.near.as_ref();
            (
                n.map(|c| c.chain.clone())
                    .unwrap_or_else(|| "mainnet".to_string()),
                n.map(|c| c.provider).unwrap_or_default(),
                n.and_then(|c| c.fastnear_api_key.clone()),
                n.and_then(|c| c.fastnear_num_threads),
                n.map(|c| c.start_height).unwrap_or(0),
                n.and_then(|c| c.concurrency).unwrap_or(6), // Default to 6 for parallel processing (conservative with large connection pool)
                // Default small preload to avoid upstream buffering
                n.and_then(|c| c.preload_pool_size).unwrap_or(1),
            )
        }
        _ => (
            "mainnet".to_string(),
            NearStreamProvider::Lake,
            None,
            None,
            0,
            6,
            1,
        ),
    };

    if !matches!(chain.as_str(), "mainnet" | "testnet") {
        return Err(anyhow::anyhow!(
            "Unsupported NEAR chain '{}'. Expected 'mainnet' or 'testnet'",
            chain
        ));
    }

    let configured_concurrency = if raw_concurrency == 0 {
        warn!(
            configured_concurrency = raw_concurrency,
            adjusted_concurrency = 1,
            "Invalid concurrency configured, adjusting to minimum 1"
        );
        1
    } else {
        raw_concurrency
    };

    let configured_preload_pool_size = if raw_preload_pool_size == 0 {
        warn!(
            configured_preload_pool_size = raw_preload_pool_size,
            adjusted_preload_pool_size = 1,
            "Invalid preload pool size configured, adjusting to minimum 1"
        );
        1
    } else {
        raw_preload_pool_size
    };

    let module_start_heights = Arc::new(configured_module_start_heights(&config));

    let stream_assignments = plan_stream_assignments(registry.as_ref());
    let stream_count = stream_assignments.len();
    let stream_plan: Vec<String> = stream_assignments
        .iter()
        .map(|assignment| {
            format!(
                "{} => {}",
                assignment.stream_id,
                assignment.module_names.join(",")
            )
        })
        .collect();

    let startup_elapsed = startup_time.elapsed();
    info!(
        chain = %chain,
        provider = configured_provider.as_str(),
        fastnear_api_key_set = configured_fastnear_api_key.is_some(),
        fastnear_num_threads = configured_fastnear_num_threads,
        stream_count = stream_count,
        stream_plan = ?stream_plan,
        configured_concurrency = configured_concurrency,
        configured_start_height = configured_start_height,
        module_start_heights = ?module_start_heights,
        configured_preload_pool_size = configured_preload_pool_size,
        enabled_modules = enabled_count,
        startup_ms = startup_elapsed.as_millis(),
        "NEAR indexer startup completed, starting stream workers"
    );

    let worker_handles = stream_assignments
        .into_iter()
        .map(|assignment| {
            tokio::spawn(run_stream_worker(
                registry.clone(),
                db.clone(),
                chain.clone(),
                configured_provider,
                configured_fastnear_api_key.clone(),
                configured_fastnear_num_threads,
                configured_start_height,
                module_start_heights.clone(),
                configured_concurrency,
                configured_preload_pool_size,
                assignment,
            ))
        })
        .collect::<Vec<_>>();

    try_join_all(worker_handles)
        .await
        .context("stream worker join failure")?
        .into_iter()
        .collect::<Result<Vec<_>>>()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{plan_stream_assignments_from_modes, NearModuleStreamMode, StreamAssignment};

    fn resolve_stream_start_height_for_test(
        configured_start_height: u64,
        module_cursors: &[Option<i64>],
        module_start_heights: &HashMap<String, u64>,
        module_names: &[&str],
    ) -> u64 {
        assert_eq!(module_cursors.len(), module_names.len());
        let mut stream_start_height: Option<u64> = None;

        for (index, cursor) in module_cursors.iter().enumerate() {
            let module_name = module_names[index];
            let module_explicit_start_height = module_start_heights.get(module_name).copied();

            let module_start_height = if let Some(height) = cursor {
                height.saturating_add(1).max(0) as u64
            } else {
                module_explicit_start_height.unwrap_or(configured_start_height)
            };

            stream_start_height = Some(
                stream_start_height
                    .map(|current| current.min(module_start_height))
                    .unwrap_or(module_start_height),
            );
        }

        stream_start_height.unwrap_or(configured_start_height)
    }

    fn resolve_assignment_start_for_test(
        assignment: &StreamAssignment,
        configured_start_height: u64,
        module_start_heights: &HashMap<String, u64>,
        module_cursors: &HashMap<String, Option<i64>>,
    ) -> u64 {
        let module_names: Vec<&str> = assignment.module_names.iter().map(String::as_str).collect();
        let cursors: Vec<Option<i64>> = assignment
            .module_names
            .iter()
            .map(|name| module_cursors.get(name).copied().flatten())
            .collect();

        resolve_stream_start_height_for_test(
            configured_start_height,
            &cursors,
            module_start_heights,
            &module_names,
        )
    }

    #[test]
    fn stream_start_uses_min_cursor_plus_one_when_all_modules_resuming() {
        let start = resolve_stream_start_height_for_test(
            0,
            &[Some(120), Some(95), Some(300)],
            &HashMap::new(),
            &["raw", "accounts", "tokens"],
        );
        assert_eq!(start, 96);
    }

    #[test]
    fn stream_start_uses_config_for_modules_without_cursor() {
        let start = resolve_stream_start_height_for_test(
            500,
            &[Some(900), None, Some(800)],
            &HashMap::new(),
            &["raw", "accounts", "tokens"],
        );
        assert_eq!(start, 500);
    }

    #[test]
    fn stream_start_uses_module_override_without_cursor() {
        let mut module_start_heights = HashMap::new();
        module_start_heights.insert("venear_governance".to_string(), 160_000_000);

        let start = resolve_stream_start_height_for_test(
            0,
            &[Some(220_000_000), None],
            &module_start_heights,
            &["tokens", "venear_governance"],
        );

        assert_eq!(start, 160_000_000);
    }

    #[test]
    fn stream_start_ignores_module_override_when_cursor_exists() {
        let mut module_start_heights = HashMap::new();
        module_start_heights.insert("venear_governance".to_string(), 160_000_000);

        let start = resolve_stream_start_height_for_test(
            0,
            &[Some(170_000_000)],
            &module_start_heights,
            &["venear_governance"],
        );

        assert_eq!(start, 170_000_001);
    }

    #[test]
    fn assignment_defaults_all_modules_to_shared_stream() {
        let assignments = plan_stream_assignments_from_modes(&[
            ("raw".to_string(), NearModuleStreamMode::Shared),
            ("accounts".to_string(), NearModuleStreamMode::Shared),
            ("tokens".to_string(), NearModuleStreamMode::Shared),
        ]);

        assert_eq!(
            assignments,
            vec![StreamAssignment {
                stream_id: "shared".to_string(),
                module_names: vec![
                    "raw".to_string(),
                    "accounts".to_string(),
                    "tokens".to_string()
                ]
            }]
        );
    }

    #[test]
    fn assignment_creates_independent_streams_for_selected_modules() {
        let assignments = plan_stream_assignments_from_modes(&[
            ("raw".to_string(), NearModuleStreamMode::Shared),
            ("tokens".to_string(), NearModuleStreamMode::Independent),
            (
                "venear_governance".to_string(),
                NearModuleStreamMode::Independent,
            ),
            ("accounts".to_string(), NearModuleStreamMode::Shared),
        ]);

        assert_eq!(
            assignments,
            vec![
                StreamAssignment {
                    stream_id: "shared".to_string(),
                    module_names: vec!["raw".to_string(), "accounts".to_string()],
                },
                StreamAssignment {
                    stream_id: "module:tokens".to_string(),
                    module_names: vec!["tokens".to_string()],
                },
                StreamAssignment {
                    stream_id: "module:venear_governance".to_string(),
                    module_names: vec!["venear_governance".to_string()],
                },
            ]
        );
    }

    #[test]
    fn shared_stream_start_uses_earliest_module_override() {
        let assignments = plan_stream_assignments_from_modes(&[
            ("raw".to_string(), NearModuleStreamMode::Shared),
            ("accounts".to_string(), NearModuleStreamMode::Shared),
        ]);

        let assignment = assignments
            .iter()
            .find(|assignment| assignment.stream_id == "shared")
            .expect("shared assignment should exist");

        let mut module_start_heights = HashMap::new();
        module_start_heights.insert("raw".to_string(), 120);
        module_start_heights.insert("accounts".to_string(), 250);

        let start_height = resolve_assignment_start_for_test(
            assignment,
            1_000,
            &module_start_heights,
            &HashMap::new(),
        );

        assert_eq!(start_height, 120);
    }

    #[test]
    fn mixed_stream_modes_resolve_starts_per_assignment() {
        let assignments = plan_stream_assignments_from_modes(&[
            ("raw".to_string(), NearModuleStreamMode::Shared),
            ("accounts".to_string(), NearModuleStreamMode::Shared),
            ("tokens".to_string(), NearModuleStreamMode::Independent),
            (
                "venear_governance".to_string(),
                NearModuleStreamMode::Independent,
            ),
        ]);

        let mut module_start_heights = HashMap::new();
        module_start_heights.insert("raw".to_string(), 400);
        module_start_heights.insert("accounts".to_string(), 800);
        module_start_heights.insert("tokens".to_string(), 1_500);
        module_start_heights.insert("venear_governance".to_string(), 160_000_000);

        let mut module_cursors = HashMap::new();
        module_cursors.insert("tokens".to_string(), Some(1_550));

        let shared_assignment = assignments
            .iter()
            .find(|assignment| assignment.stream_id == "shared")
            .expect("shared assignment should exist");
        let tokens_assignment = assignments
            .iter()
            .find(|assignment| assignment.stream_id == "module:tokens")
            .expect("tokens independent assignment should exist");
        let venear_assignment = assignments
            .iter()
            .find(|assignment| assignment.stream_id == "module:venear_governance")
            .expect("venear independent assignment should exist");

        let shared_start = resolve_assignment_start_for_test(
            shared_assignment,
            0,
            &module_start_heights,
            &module_cursors,
        );
        let tokens_start = resolve_assignment_start_for_test(
            tokens_assignment,
            0,
            &module_start_heights,
            &module_cursors,
        );
        let venear_start = resolve_assignment_start_for_test(
            venear_assignment,
            0,
            &module_start_heights,
            &module_cursors,
        );

        assert_eq!(shared_start, 400);
        assert_eq!(tokens_start, 1_551);
        assert_eq!(venear_start, 160_000_000);
    }

    #[test]
    fn shared_stream_includes_unset_module_global_start_in_minimum() {
        let assignments = plan_stream_assignments_from_modes(&[
            ("raw".to_string(), NearModuleStreamMode::Shared),
            ("accounts".to_string(), NearModuleStreamMode::Shared),
        ]);

        let assignment = assignments
            .iter()
            .find(|assignment| assignment.stream_id == "shared")
            .expect("shared assignment should exist");

        let mut module_start_heights = HashMap::new();
        module_start_heights.insert("raw".to_string(), 160_000_000);

        let start_height = resolve_assignment_start_for_test(
            assignment,
            0,
            &module_start_heights,
            &HashMap::new(),
        );

        assert_eq!(start_height, 0);
    }

    #[test]
    fn shared_stream_start_height_combinations() {
        let assignment = StreamAssignment {
            stream_id: "shared".to_string(),
            module_names: vec!["raw".to_string(), "accounts".to_string()],
        };

        struct Case {
            name: &'static str,
            configured_start_height: u64,
            module_start_heights: Vec<(&'static str, u64)>,
            module_cursors: Vec<(&'static str, Option<i64>)>,
            expected_start_height: u64,
        }

        let cases = vec![
            Case {
                name: "no overrides use global zero",
                configured_start_height: 0,
                module_start_heights: vec![],
                module_cursors: vec![],
                expected_start_height: 0,
            },
            Case {
                name: "no overrides use global non-zero",
                configured_start_height: 500,
                module_start_heights: vec![],
                module_cursors: vec![],
                expected_start_height: 500,
            },
            Case {
                name: "single explicit override with unset peer uses lower global",
                configured_start_height: 0,
                module_start_heights: vec![("raw", 160_000_000)],
                module_cursors: vec![],
                expected_start_height: 0,
            },
            Case {
                name: "single explicit override lower than global wins minimum",
                configured_start_height: 300,
                module_start_heights: vec![("raw", 200)],
                module_cursors: vec![],
                expected_start_height: 200,
            },
            Case {
                name: "single explicit override higher than global loses minimum",
                configured_start_height: 100,
                module_start_heights: vec![("raw", 200)],
                module_cursors: vec![],
                expected_start_height: 100,
            },
            Case {
                name: "two explicit overrides use minimum",
                configured_start_height: 0,
                module_start_heights: vec![("raw", 400), ("accounts", 250)],
                module_cursors: vec![],
                expected_start_height: 250,
            },
            Case {
                name: "cursor beats explicit when lower",
                configured_start_height: 0,
                module_start_heights: vec![("raw", 400), ("accounts", 800)],
                module_cursors: vec![("accounts", Some(199))],
                expected_start_height: 200,
            },
            Case {
                name: "explicit beats cursor when lower",
                configured_start_height: 0,
                module_start_heights: vec![("raw", 120), ("accounts", 800)],
                module_cursors: vec![("accounts", Some(250))],
                expected_start_height: 120,
            },
            Case {
                name: "cursor plus unset without overrides uses global minimum",
                configured_start_height: 500,
                module_start_heights: vec![],
                module_cursors: vec![("raw", Some(900))],
                expected_start_height: 500,
            },
            Case {
                name: "explicit module with cursor uses cursor",
                configured_start_height: 0,
                module_start_heights: vec![("raw", 600), ("accounts", 900)],
                module_cursors: vec![("raw", Some(750))],
                expected_start_height: 751,
            },
            Case {
                name: "two cursors use the lower next height",
                configured_start_height: 0,
                module_start_heights: vec![],
                module_cursors: vec![("raw", Some(2)), ("accounts", Some(9))],
                expected_start_height: 3,
            },
            Case {
                name: "negative cursor resumes from zero",
                configured_start_height: 500,
                module_start_heights: vec![],
                module_cursors: vec![("raw", Some(-1))],
                expected_start_height: 0,
            },
        ];

        for case in cases {
            let module_start_heights: HashMap<String, u64> = case
                .module_start_heights
                .iter()
                .map(|(name, height)| ((*name).to_string(), *height))
                .collect();
            let module_cursors: HashMap<String, Option<i64>> = case
                .module_cursors
                .iter()
                .map(|(name, cursor)| ((*name).to_string(), *cursor))
                .collect();

            let start_height = resolve_assignment_start_for_test(
                &assignment,
                case.configured_start_height,
                &module_start_heights,
                &module_cursors,
            );

            assert_eq!(
                start_height, case.expected_start_height,
                "case '{}' failed",
                case.name
            );
        }
    }

    #[test]
    fn independent_stream_start_height_combinations() {
        let assignment = StreamAssignment {
            stream_id: "module:venear_governance".to_string(),
            module_names: vec!["venear_governance".to_string()],
        };

        struct Case {
            name: &'static str,
            configured_start_height: u64,
            module_start_height: Option<u64>,
            cursor: Option<i64>,
            expected_start_height: u64,
        }

        let cases = vec![
            Case {
                name: "module override only",
                configured_start_height: 0,
                module_start_height: Some(160_000_000),
                cursor: None,
                expected_start_height: 160_000_000,
            },
            Case {
                name: "module override can be lower than global",
                configured_start_height: 500,
                module_start_height: Some(200),
                cursor: None,
                expected_start_height: 200,
            },
            Case {
                name: "module override can be higher than global",
                configured_start_height: 100,
                module_start_height: Some(200),
                cursor: None,
                expected_start_height: 200,
            },
            Case {
                name: "global only",
                configured_start_height: 500,
                module_start_height: None,
                cursor: None,
                expected_start_height: 500,
            },
            Case {
                name: "cursor overrides module start",
                configured_start_height: 0,
                module_start_height: Some(160_000_000),
                cursor: Some(170_000_000),
                expected_start_height: 170_000_001,
            },
            Case {
                name: "cursor overrides global",
                configured_start_height: 500,
                module_start_height: None,
                cursor: Some(700),
                expected_start_height: 701,
            },
            Case {
                name: "negative cursor resumes from zero",
                configured_start_height: 500,
                module_start_height: None,
                cursor: Some(-1),
                expected_start_height: 0,
            },
        ];

        for case in cases {
            let mut module_start_heights = HashMap::new();
            if let Some(start_height) = case.module_start_height {
                module_start_heights.insert("venear_governance".to_string(), start_height);
            }

            let mut module_cursors = HashMap::new();
            if case.cursor.is_some() {
                module_cursors.insert("venear_governance".to_string(), case.cursor);
            }

            let start_height = resolve_assignment_start_for_test(
                &assignment,
                case.configured_start_height,
                &module_start_heights,
                &module_cursors,
            );

            assert_eq!(
                start_height, case.expected_start_height,
                "case '{}' failed",
                case.name
            );
        }
    }
}
