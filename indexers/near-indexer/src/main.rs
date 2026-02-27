use anyhow::{Context, Result};
use common::config::{AppConfig, NearModuleStreamMode, ToLog};
use common::telemetry::{self, TelemetrySetup};
use db_core::connection::from_config as db_from_config;
use futures::future::try_join_all;
use near_indexer::{modules, ModuleRegistry};
use std::collections::HashSet;
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

async fn resolve_stream_start_height(
    db: &sea_orm::DatabaseConnection,
    stream_id: &str,
    module_names: &[String],
    configured_start_height: u64,
) -> Result<(u64, bool)> {
    let mut stream_start_height: Option<u64> = None;
    let mut any_module_has_cursor = false;

    for module_name in module_names {
        let cursor = near_indexer::core::cursor::get_module_cursor(db, module_name).await?;

        let module_start_height = if let Some(height) = cursor {
            let next_height = (height.max(0) as u64).saturating_add(1);
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
            if configured_start_height > 0 {
                // Seed in-memory cursor so out-of-order block completion still advances
                // contiguous cursors correctly from the configured start.
                let seed_height =
                    i64::try_from(configured_start_height.saturating_sub(1)).unwrap_or(i64::MAX);
                near_indexer::core::cursor::seed_module_cursor(module_name, seed_height);
            }

            info!(
                stream_id = stream_id,
                module = module_name,
                configured_start_height,
                "No cursor found, will start from configured height"
            );

            configured_start_height
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

async fn run_stream_worker(
    registry: Arc<ModuleRegistry>,
    db: Arc<sea_orm::DatabaseConnection>,
    chain: String,
    configured_start_height: u64,
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

        let (streamer_handle, stream) =
            match near_indexer::core::lake_stream::create_lake_stream(lake_config) {
                Ok(stream_parts) => stream_parts,
                Err(err) => {
                    error!(
                        stream_id = %assignment.stream_id,
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
                        "Lake streamer exited unexpectedly, restarting worker"
                    ),
                    Ok(Err(err)) => error!(
                        stream_id = %assignment.stream_id,
                        error = %err,
                        "Lake streamer failed, restarting worker"
                    ),
                    Err(join_err) => error!(
                        stream_id = %assignment.stream_id,
                        error = %join_err,
                        "Lake streamer panicked, restarting worker"
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

    let telemetry_setup = TelemetrySetup::from_config(
        "near-indexer",
        config.telemetry(),
        &config.logging().level,
        config.logging().format.as_deref(),
    )?;
    let _guard = telemetry::init_telemetry(telemetry_setup)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Failed to create Tokio runtime")?;

    if let Err(err) = runtime.block_on(run(config)) {
        sentry::integrations::anyhow::capture_anyhow(&err);
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

    let (chain, configured_start_height, configured_concurrency, configured_preload_pool_size) =
        match &config {
            AppConfig::Indexer(idx) => {
                let idx = idx.as_ref();
                let n = idx.near.as_ref();
                (
                    n.map(|c| c.chain.clone())
                        .unwrap_or_else(|| "mainnet".to_string()),
                    n.map(|c| c.start_height).unwrap_or(0),
                    n.and_then(|c| c.concurrency).unwrap_or(6), // Default to 6 for parallel processing (conservative with large connection pool)
                    // Default small preload to avoid upstream buffering
                    n.and_then(|c| c.preload_pool_size).unwrap_or(1),
                )
            }
            _ => ("mainnet".to_string(), 0, 6, 1),
        };

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
        stream_count = stream_count,
        stream_plan = ?stream_plan,
        configured_concurrency = configured_concurrency,
        configured_start_height = configured_start_height,
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
                configured_start_height,
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
    use super::{plan_stream_assignments_from_modes, NearModuleStreamMode, StreamAssignment};

    fn resolve_stream_start_height_for_test(
        configured_start_height: u64,
        module_cursors: &[Option<i64>],
    ) -> u64 {
        let mut stream_start_height: Option<u64> = None;

        for cursor in module_cursors {
            let module_start_height = if let Some(height) = cursor {
                let non_negative = (*height).max(0) as u64;
                non_negative.saturating_add(1)
            } else {
                configured_start_height
            };

            stream_start_height = Some(
                stream_start_height
                    .map(|current| current.min(module_start_height))
                    .unwrap_or(module_start_height),
            );
        }

        stream_start_height.unwrap_or(configured_start_height)
    }

    #[test]
    fn stream_start_uses_min_cursor_plus_one_when_all_modules_resuming() {
        let start = resolve_stream_start_height_for_test(0, &[Some(120), Some(95), Some(300)]);
        assert_eq!(start, 96);
    }

    #[test]
    fn stream_start_uses_config_for_modules_without_cursor() {
        let start = resolve_stream_start_height_for_test(500, &[Some(900), None, Some(800)]);
        assert_eq!(start, 500);
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
}
