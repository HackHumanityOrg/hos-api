use anyhow::{Context, Result};
use common::config::{AppConfig, ToLog};
use common::telemetry::{self, TelemetrySetup};
use discourse_indexer::sync as services;
use discourse_indexer::sync::Concurrency;
use opentelemetry::KeyValue;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use tracing::{error, info, instrument};

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Load indexer-specific configuration (type-safe)
    let config =
        AppConfig::load_indexer("discourse-indexer").context("Failed to load configuration")?;

    // Initialize telemetry (Dash0 if configured, basic tracing otherwise)
    let telemetry_setup = TelemetrySetup::from_config(
        "discourse-indexer",
        config.telemetry(),
        &config.logging().level,
        config.logging().format.as_deref(),
    )
    .context("Failed to create telemetry setup")?;

    let _telemetry_guard =
        telemetry::init_telemetry(telemetry_setup).context("Failed to initialize telemetry")?;

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

    // Initialize database connection
    let db_result = db_core::connection::from_config(config.database()).await;

    match &db_result {
        Ok(_) => {
            info!("Database connection established successfully");
        }
        Err(e) => {
            tracing::warn!("Failed to connect to database: {}", e);
            info!("Starting indexer without database connection - will retry when needed");
        }
    }

    run_indexing_loop(&config).await?;

    Ok(())
}

#[instrument(skip(config))]
async fn run_indexing_loop(config: &AppConfig) -> Result<()> {
    use tokio::time::{interval_at, Duration, Instant, MissedTickBehavior};

    const INITIAL_FULL_DELAY_SECS: u64 = 5;
    const INITIAL_RECENT_DELAY_SECS: u64 = 5;

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

    let db = match db_core::connection::from_config(config.database()).await {
        Ok(db) => Arc::new(db),
        Err(e) => {
            error!(error = %e, "DB connect failed; retrying shortly");
            tokio::time::sleep(Duration::from_secs(5)).await;
            Arc::new(db_core::connection::from_config(config.database()).await?)
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

        let post_tasks_limit = inst.max_post_tasks.unwrap_or(10);
        let likes_limit = inst.likes_concurrency.unwrap_or(10);
        let revs_limit = inst.revisions_concurrency.unwrap_or(5);
        let recent_topic_cap = inst.recent_topic_pages_cap.unwrap_or(3);
        let conc = Concurrency::new(post_tasks_limit, likes_limit, revs_limit);

        discourse_indexer::domain::instances::repo::register_instance(
            db.as_ref(),
            &instance_key,
            &base_url,
        )
        .await
        .ok();

        let client = Arc::new(discourse_indexer::discourse_api::DiscourseClient::new(
            base_url.clone(),
        ));
        let full_running = Arc::new(AtomicBool::new(false));
        let recent_running = Arc::new(AtomicBool::new(false));

        {
            let db_recent = db.clone();
            let client_recent = client.clone();
            let conc_recent = conc.clone();
            let instance_recent = instance_key.clone();
            let recent_flag = recent_running.clone();
            handles.push(tokio::spawn(async move {
                let start = Instant::now() + Duration::from_secs(INITIAL_RECENT_DELAY_SECS);
                let mut interval = interval_at(start, Duration::from_secs(recent_every));
                interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

                loop {
                    interval.tick().await;

                    if recent_flag
                        .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                        .is_err()
                    {
                        info!(instance = %instance_recent, "Recent updates already running, skipping this cycle");
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
    }

    for handle in handles {
        let _ = handle.await;
    }

    Ok(())
}
