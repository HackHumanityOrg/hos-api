use anyhow::{Context, Result};
use common::config::{AppConfig, ToLog};
use common::telemetry::{self, KeyValue, TelemetrySetup};
use discourse_indexer::sync as services;
use discourse_indexer::sync::Concurrency;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tracing::{error, info, instrument, warn};

fn main() -> Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Load indexer-specific configuration (type-safe)
    let config =
        AppConfig::load_indexer("discourse-indexer").context("Failed to load configuration")?;

    // Initialize telemetry and tracing
    let telemetry_setup = TelemetrySetup::from_config(
        "discourse-indexer",
        config.telemetry(),
        &config.logging().level,
    )
    .context("Failed to create telemetry setup")?;

    let _telemetry_guard =
        telemetry::init_telemetry(telemetry_setup).context("Failed to initialize telemetry")?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Failed to create Tokio runtime")?;

    if let Err(err) = runtime.block_on(run(config)) {
        error!(error = ?err, "Discourse indexer terminated with error");
        return Err(err);
    }

    Ok(())
}

async fn run(config: AppConfig) -> Result<()> {
    info!("Starting Indexer 1 Service...");
    // Log sanitized configuration
    info!(config = %config.to_log(), "Configuration loaded");

    // Record indexer startup metric
    if config.telemetry().enabled {
        telemetry::increment_counter(
            "indexer.startup",
            &[
                KeyValue::new("service", "discourse-indexer"),
                KeyValue::new("type", "primary"),
            ],
        );
    }

    let db = connect_db_with_retry(config.database()).await;

    run_indexing_loop(&config, db).await?;

    Ok(())
}

#[instrument(skip(database_config))]
async fn connect_db_with_retry(
    database_config: &common::config::DatabaseConfig,
) -> Arc<sea_orm::DatabaseConnection> {
    let mut attempt: u32 = 1;

    loop {
        match db_core::connection::from_config(database_config).await {
            Ok(db) => {
                info!(attempt, "Database connection established successfully");
                return Arc::new(db);
            }
            Err(e) => {
                warn!(
                    attempt,
                    error = %e,
                    retry_secs = 5,
                    "DB connect failed; retrying shortly"
                );
                attempt = attempt.saturating_add(1);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        }
    }
}

#[instrument(skip(config, db))]
async fn run_indexing_loop(config: &AppConfig, db: Arc<sea_orm::DatabaseConnection>) -> Result<()> {
    use tokio::time::{interval_at, Duration, Instant, MissedTickBehavior};

    const INITIAL_FULL_DELAY_SECS: u64 = 5;
    const INITIAL_RECENT_DELAY_SECS: u64 = 5;
    const INITIAL_DIRECTORY_DELAY_SECS: u64 = 10;

    info!("Starting Discourse multi-instance indexing loop");

    let discourse = match config.discourse() {
        Some(d) => d.clone(),
        None => {
            info!("No [discourse] config found; idle loop");
            loop {
                tokio::time::sleep(Duration::from_secs(30)).await;
            }
        }
    };

    let mut handles = Vec::new();
    for inst in discourse.instances.iter().filter(|i| i.enabled) {
        let instance_key = inst.key.clone();
        let base_url = inst.base_url.clone();
        let full_every = inst
            .full_refresh_interval_sec
            .unwrap_or(discourse.full_refresh_interval_sec);
        let recent_every = inst
            .recent_update_interval_sec
            .unwrap_or(discourse.recent_update_interval_sec);
        let max_pages = discourse.max_pages_per_run;
        let lookback_hours = discourse.recent_lookback_hours;
        let telemetry_enabled = config.telemetry().enabled;

        let http_limit = inst
            .http_max_concurrency
            .unwrap_or(discourse.http_max_concurrency);
        let post_tasks_limit = inst.max_post_tasks.unwrap_or(discourse.max_post_tasks);
        let likes_limit = inst
            .likes_concurrency
            .unwrap_or(discourse.likes_concurrency);
        let revs_limit = inst
            .revisions_concurrency
            .unwrap_or(discourse.revisions_concurrency);
        let recent_topic_cap = inst
            .recent_topic_pages_cap
            .unwrap_or(discourse.recent_topic_pages_cap);
        let metrics_refresh_batch_size = inst
            .metrics_refresh_batch_size
            .unwrap_or(discourse.metrics_refresh_batch_size);
        let metrics_active_window_days = inst
            .metrics_active_window_days
            .unwrap_or(discourse.metrics_active_window_days);
        let metrics_active_refresh_hours = inst
            .metrics_active_refresh_hours
            .unwrap_or(discourse.metrics_active_refresh_hours);
        let conc = Concurrency::new(post_tasks_limit, likes_limit, revs_limit);

        discourse_indexer::domain::instances::repo::register_instance(
            db.as_ref(),
            &instance_key,
            &base_url,
        )
        .await
        .ok();

        let client = Arc::new(
            discourse_indexer::discourse_api::DiscourseClient::new(base_url.clone())
                .with_http_max_concurrency(http_limit),
        );
        let full_running = Arc::new(AtomicBool::new(false));
        let recent_running = Arc::new(AtomicBool::new(false));
        let directory_running = Arc::new(AtomicBool::new(false));
        let full_completed_once = Arc::new(AtomicBool::new(false));

        {
            let db_recent = db.clone();
            let client_recent = client.clone();
            let conc_recent = conc.clone();
            let instance_recent = instance_key.clone();
            let recent_flag = recent_running.clone();
            let full_flag_recent = full_running.clone();
            let full_done_recent = full_completed_once.clone();
            handles.push(tokio::spawn(async move {
                let start = Instant::now() + Duration::from_secs(INITIAL_RECENT_DELAY_SECS);
                let mut interval = interval_at(start, Duration::from_secs(recent_every));
                interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

                loop {
                    interval.tick().await;

                    if !full_done_recent.load(Ordering::Acquire)
                        && full_flag_recent.load(Ordering::Acquire)
                    {
                        info!(
                            instance = %instance_recent,
                            "Initial full refresh still running, skipping recent cycle"
                        );
                        continue;
                    }

                    if recent_flag
                        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                        .is_err()
                    {
                        info!(instance = %instance_recent, "Recent updates already running, skipping this cycle");
                        continue;
                    }

                    if !full_done_recent.load(Ordering::Acquire)
                        && full_flag_recent.load(Ordering::Acquire)
                    {
                        info!(
                            instance = %instance_recent,
                            "Initial full refresh started while scheduling recent cycle, skipping"
                        );
                        recent_flag.store(false, Ordering::Release);
                        continue;
                    }

                    let cycle_start = Instant::now();
                    let result = services::sync_recent(
                        db_recent.clone(),
                        client_recent.as_ref(),
                        &instance_recent,
                        recent_topic_cap,
                        &conc_recent,
                        max_pages,
                        lookback_hours,
                    )
                    .await;

                    match result {
                        Ok(_) => {
                            info!(instance = %instance_recent, duration = ?cycle_start.elapsed(), "Recent updates completed successfully");
                            if telemetry_enabled {
                                telemetry::increment_counter(
                                    "indexer.batches.processed",
                                    &[
                                        KeyValue::new("indexer", "discourse-indexer"),
                                        KeyValue::new("kind", "recent"),
                                        KeyValue::new("instance", instance_recent.clone()),
                                    ],
                                );
                            }
                        }
                        Err(e) => {
                            error!(instance = %instance_recent, duration = ?cycle_start.elapsed(), error = ?e, "Recent updates completed with errors");
                        }
                    }

                    recent_flag.store(false, Ordering::Release);
                }
            }));
        }

        {
            let db_full = db.clone();
            let client_full = client.clone();
            let conc_full = conc.clone();
            let instance_full = instance_key.clone();
            let full_flag = full_running.clone();
            let full_done_flag = full_completed_once.clone();
            handles.push(tokio::spawn(async move {
                let start = Instant::now() + Duration::from_secs(INITIAL_FULL_DELAY_SECS);
                let mut interval = interval_at(start, Duration::from_secs(full_every));
                interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

                loop {
                    interval.tick().await;

                    if full_flag
                        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                        .is_err()
                    {
                        info!(instance = %instance_full, "Full refresh already running, skipping this cycle");
                        continue;
                    }

                    let cycle_start = Instant::now();
                    let result = services::sync_full(
                        db_full.clone(),
                        client_full.as_ref(),
                        &instance_full,
                        &conc_full,
                        max_pages,
                    )
                    .await;

                    match result {
                        Ok(_) => {
                            full_done_flag.store(true, Ordering::Release);
                            info!(instance = %instance_full, duration = ?cycle_start.elapsed(), "Full refresh completed successfully");
                            if telemetry_enabled {
                                telemetry::increment_counter(
                                    "indexer.batches.processed",
                                    &[
                                        KeyValue::new("indexer", "discourse-indexer"),
                                        KeyValue::new("kind", "full"),
                                        KeyValue::new("instance", instance_full.clone()),
                                    ],
                                );
                            }
                        }
                        Err(e) => {
                            error!(instance = %instance_full, duration = ?cycle_start.elapsed(), error = ?e, "Full refresh completed with errors");
                        }
                    }

                    full_flag.store(false, Ordering::Release);
                }
            }));
        }

        {
            let db_directory = db.clone();
            let client_directory = client.clone();
            let instance_directory = instance_key.clone();
            let directory_flag = directory_running.clone();
            handles.push(tokio::spawn(async move {
                let start = Instant::now() + Duration::from_secs(INITIAL_DIRECTORY_DELAY_SECS);
                let mut interval = interval_at(start, Duration::from_secs(recent_every));
                interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

                loop {
                    interval.tick().await;

                    if directory_flag
                        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                        .is_err()
                    {
                        info!(instance = %instance_directory, "Directory users sync already running, skipping this cycle");
                        continue;
                    }

                    let cycle_start = Instant::now();
                    let result = services::sync_directory_full(
                        db_directory.clone(),
                        client_directory.as_ref(),
                        &instance_directory,
                        services::MetricsRefreshConfig {
                            cycle_interval_sec: recent_every,
                            batch_size: metrics_refresh_batch_size,
                            active_window_days: metrics_active_window_days,
                            active_refresh_hours: metrics_active_refresh_hours,
                        },
                    )
                    .await;

                    match result {
                        Ok(_) => {
                            info!(instance = %instance_directory, duration = ?cycle_start.elapsed(), "Directory users sync completed successfully");
                            if telemetry_enabled {
                                telemetry::increment_counter(
                                    "indexer.batches.processed",
                                    &[
                                        KeyValue::new("indexer", "discourse-indexer"),
                                        KeyValue::new("kind", "directory"),
                                        KeyValue::new("instance", instance_directory.clone()),
                                    ],
                                );
                            }
                        }
                        Err(e) => {
                            error!(instance = %instance_directory, duration = ?cycle_start.elapsed(), error = ?e, "Directory users sync completed with errors");
                        }
                    }

                    directory_flag.store(false, Ordering::Release);
                }
            }));
        }
    }

    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}
