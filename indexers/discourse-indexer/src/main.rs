use anyhow::{Context, Result};
use common::config::{AppConfig, ToLog};
use common::telemetry::{self, TelemetrySetup};
use discourse_indexer::sync as services;
use discourse_indexer::sync::Concurrency;
use opentelemetry::KeyValue;
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
    info!("Starting Discourse multi-instance indexing loop");

    let discourse = match config.discourse() {
        Some(d) => d.clone(),
        None => {
            info!("No [discourse] config found; idle loop");
            loop {
                tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            }
        }
    };

    let db = match db_core::connection::from_config(config.database()).await {
        Ok(db) => db,
        Err(e) => {
            error!(error = %e, "DB connect failed; retrying shortly");
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            db_core::connection::from_config(config.database()).await?
        }
    };

    let mut handles = Vec::new();
    for inst in discourse.instances.iter().filter(|i| i.enabled) {
        let instance_key = inst.key.clone();
        let base_url = inst.base_url.clone();
        // Allow per-instance overrides; fall back to global
        let full_every = inst
            .full_refresh_interval_sec
            .unwrap_or(discourse.full_refresh_interval_sec);
        let recent_every = inst
            .recent_update_interval_sec
            .unwrap_or(discourse.recent_update_interval_sec);
        let _max_pages = discourse.max_pages_per_run;

        let db_clone = db.clone();
        let cfg_telemetry = config.telemetry().enabled;

        // Per-instance tuning with defaults
        let http_limit = inst.max_http_concurrency.unwrap_or(8);
        let post_tasks_limit = inst.max_post_tasks.unwrap_or(10);
        let likes_limit = inst.likes_concurrency.unwrap_or(10);
        let revs_limit = inst.revisions_concurrency.unwrap_or(5);
        // Per-instance recent caps (defaults: topics=3, posts=2)
        let recent_topic_cap = inst.recent_topic_pages_cap.unwrap_or(3);
        let conc = Concurrency::new(post_tasks_limit, likes_limit, revs_limit);

        // Register instance once
        {
            let _ = discourse_indexer::domain::instances::repo::register_instance(
                &db_clone,
                &instance_key,
                &base_url,
            )
            .await;
        }

        // Spawn recent loop task (non-overlapping within kind)
        {
            let db_clone = db.clone();
            let instance_key = instance_key.clone();
            let conc = conc.clone();
            let base_url_recent = base_url.clone();
            let http_limit_recent = http_limit;
            handles.push(tokio::spawn(async move {
                use tokio::time::{interval_at, Duration, Instant};
                let client = discourse_indexer::discourse_api::DiscourseClient::new_with_limits(
                    base_url_recent.clone(),
                    5,
                    Some(http_limit_recent),
                );
                let start = Instant::now() + Duration::from_secs(3);
                let mut tick = interval_at(start, Duration::from_secs(recent_every));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    tick.tick().await;
                    if let Err(e) = services::sync_recent(
                        &db_clone,
                        &client,
                        &instance_key,
                        recent_topic_cap,
                        &conc,
                    )
                    .await
                    {
                        error!(instance = %instance_key, error = ?e, "recent indexing failed");
                    } else if cfg_telemetry {
                        telemetry::increment_counter(
                            "indexer.batches.processed",
                            &[
                                KeyValue::new("indexer", "discourse-indexer"),
                                KeyValue::new("kind", "recent"),
                                KeyValue::new("instance", instance_key.clone()),
                            ],
                        );
                    }
                }
            }));
        }

        // Spawn full loop task (non-overlapping within kind)
        {
            let db_clone = db.clone();
            let instance_key = instance_key.clone();
            let conc = conc.clone();
            let base_url_full = base_url.clone();
            let http_limit_full = http_limit;
            handles.push(tokio::spawn(async move {
                use tokio::time::{interval_at, Duration, Instant};
                let client = discourse_indexer::discourse_api::DiscourseClient::new_with_limits(
                    base_url_full.clone(),
                    5,
                    Some(http_limit_full),
                );
                let start = Instant::now() + Duration::from_secs(60);
                let mut tick = interval_at(start, Duration::from_secs(full_every));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    tick.tick().await;
                    if let Err(e) =
                        services::sync_full(&db_clone, &client, &instance_key, &conc).await
                    {
                        error!(instance = %instance_key, error = ?e, "full indexing failed");
                    } else if cfg_telemetry {
                        telemetry::increment_counter(
                            "indexer.batches.processed",
                            &[
                                KeyValue::new("indexer", "discourse-indexer"),
                                KeyValue::new("kind", "full"),
                                KeyValue::new("instance", instance_key.clone()),
                            ],
                        );
                    }
                }
            }));
        }
    }

    for h in handles {
        let _ = h.await;
    }

    Ok(())
}
