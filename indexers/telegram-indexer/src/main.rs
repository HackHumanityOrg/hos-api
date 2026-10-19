use anyhow::{Context, Result};
use common::config::{AppConfig, ToLog};
use common::telemetry::{self, TelemetrySetup};
use grammers_client::client::UpdatesConfiguration;
use grammers_client::update::Update;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use telegram_indexer::{
    ChannelManager, ChannelSyncCursor, RuntimeConfig, TelegramAuth, TelegramClient,
};

fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    let config =
        AppConfig::load_indexer("telegram-indexer").context("Failed to load indexer config")?;

    let telemetry_setup = TelemetrySetup::from_config(
        "telegram-indexer",
        config.telemetry(),
        &config.logging().level,
    )
    .context("Failed to build telemetry setup")?;

    let _guard = telemetry::init_telemetry(telemetry_setup)
        .context("Failed to initialize telemetry/tracing")?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Failed to create Tokio runtime")?;

    if let Err(err) = runtime.block_on(run(config)) {
        error!(error = ?err, "Telegram indexer terminated with error");
        return Err(err);
    }

    Ok(())
}

async fn run(config: AppConfig) -> Result<()> {
    info!(config = %config.to_log(), "Starting telegram indexer");

    let runtime_config = Arc::new(RuntimeConfig::from_app(&config)?);
    let db = connect_db_with_retry(config.database()).await;

    info!("Authenticating Telegram client");
    let mut connection = TelegramAuth::connect(runtime_config.as_ref())
        .await
        .context("Telegram authentication failed")?;
    let updates_rx = connection
        .take_updates_rx()
        .context("Failed to initialize Telegram update receiver")?;

    let mut updates = connection
        .client
        .stream_updates(
            updates_rx,
            UpdatesConfiguration {
                catch_up: true,
                ..Default::default()
            },
        )
        .await;

    let telegram_client = Arc::new(TelegramClient::new(
        connection.client.clone(),
        runtime_config.clone(),
    ));
    if !telegram_client.is_connected().await {
        anyhow::bail!("Telegram client is not authorized after authentication");
    }

    let channel_manager = ChannelManager::new(telegram_client.clone(), db.clone());
    let channels = channel_manager
        .resolve_and_ensure_channels(&runtime_config.channels)
        .await
        .context("Failed to resolve configured Telegram channels")?;

    info!(channel_count = channels.len(), "Resolved Telegram channels");

    let mut update_routes: HashMap<i64, mpsc::UnboundedSender<Update>> = HashMap::new();
    let mut channel_jobs = Vec::new();

    for (channel_db_id, chat) in channels {
        let (tx, rx) = mpsc::unbounded_channel();
        update_routes.insert(chat.id().bot_api_dialog_id(), tx);
        channel_jobs.push((channel_db_id, chat, rx));
    }

    let shutdown_token = CancellationToken::new();

    let dispatcher_routes = update_routes;
    let dispatcher_shutdown = shutdown_token.clone();
    let retry_base_delay_seconds = runtime_config.retry_base_delay_seconds;

    let dispatcher_task = tokio::spawn(async move {
        let mut consecutive_errors = 0u32;
        let max_backoff_seconds = 300u64;

        loop {
            tokio::select! {
                _ = dispatcher_shutdown.cancelled() => {
                    info!("Telegram update dispatcher shutting down");
                    break;
                }
                update_result = updates.next() => {
                    match update_result {
                        Ok(update) => {
                            if consecutive_errors > 0 {
                                info!(consecutive_errors, "Telegram update stream recovered");
                                consecutive_errors = 0;
                            }

                            match update {
                                Update::NewMessage(message) => {
                                    let Some(peer) = message.peer() else {
                                        continue;
                                    };

                                    let chat_id = peer.id().bot_api_dialog_id();
                                    let message_id = message.id();
                                    if let Some(sender) = dispatcher_routes.get(&chat_id) {
                                        if sender.send(Update::NewMessage(message)).is_err() {
                                            warn!(chat_id, message_id, "Dropped channel receiver for realtime message");
                                        }
                                    } else {
                                        debug!(chat_id, message_id, "No active channel receiver for realtime message");
                                    }
                                }
                                Update::MessageEdited(message) => {
                                    let Some(peer) = message.peer() else {
                                        continue;
                                    };

                                    let chat_id = peer.id().bot_api_dialog_id();
                                    let message_id = message.id();
                                    if let Some(sender) = dispatcher_routes.get(&chat_id) {
                                        if sender.send(Update::MessageEdited(message)).is_err() {
                                            warn!(chat_id, message_id, "Dropped channel receiver for realtime edit");
                                        }
                                    } else {
                                        debug!(chat_id, message_id, "No active channel receiver for realtime edit");
                                    }
                                }
                                _ => {}
                            }
                        }
                        Err(err) => {
                            consecutive_errors = consecutive_errors.saturating_add(1);
                            let power = consecutive_errors.saturating_sub(1);
                            let backoff = (retry_base_delay_seconds.saturating_mul(2_u64.saturating_pow(power)))
                                .min(max_backoff_seconds);

                            error!(
                                consecutive_errors,
                                backoff,
                                error = %err,
                                "Telegram update stream failed"
                            );

                            tokio::time::sleep(Duration::from_secs(backoff)).await;
                        }
                    }
                }
            }
        }
    });

    let semaphore = Arc::new(Semaphore::new(runtime_config.max_concurrent_channels));
    let mut sync_tasks: Vec<JoinHandle<()>> = Vec::new();

    for (channel_db_id, chat, update_rx) in channel_jobs {
        let db_clone = db.clone();
        let client_clone = telegram_client.clone();
        let runtime_clone = runtime_config.clone();
        let semaphore_clone = semaphore.clone();
        let task_shutdown = shutdown_token.clone();

        let task = tokio::spawn(async move {
            let channel_name = chat.name().unwrap_or("unknown").to_string();
            info!(channel = %channel_name, "Starting channel sync worker");

            let mut cursor = match ChannelSyncCursor::load_or_create(
                channel_db_id,
                chat.clone(),
                client_clone,
                db_clone,
                runtime_clone,
                update_rx,
            )
            .await
            {
                Ok(cursor) => cursor,
                Err(err) => {
                    error!(channel = %channel_name, error = %err, "Failed to initialize channel cursor");
                    return;
                }
            };

            loop {
                if task_shutdown.is_cancelled() {
                    info!(channel = %channel_name, "Stopping channel worker on shutdown signal");
                    break;
                }

                let permit: OwnedSemaphorePermit = match semaphore_clone
                    .clone()
                    .acquire_owned()
                    .await
                {
                    Ok(permit) => permit,
                    Err(err) => {
                        error!(channel = %channel_name, error = %err, "Semaphore closed unexpectedly");
                        break;
                    }
                };

                match cursor.run_sync(permit).await {
                    Ok(()) => {
                        warn!(channel = %channel_name, "Channel sync exited unexpectedly; restarting");
                    }
                    Err(err) => {
                        error!(channel = %channel_name, error = %err, "Channel sync failed; retrying");
                    }
                }

                tokio::select! {
                    _ = task_shutdown.cancelled() => {
                        info!(channel = %channel_name, "Stopping channel worker before retry");
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {}
                }
            }
        });

        sync_tasks.push(task);
    }

    info!("Telegram indexer running; waiting for shutdown signal");

    let shutdown_result: Result<(), std::io::Error> = tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Received SIGINT (Ctrl+C)");
            Ok(())
        }
        _ = async {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm = signal(SignalKind::terminate())?;
                sigterm.recv().await;
                info!("Received SIGTERM");
                Ok::<(), std::io::Error>(())
            }
            #[cfg(not(unix))]
            {
                futures::future::pending::<()>().await;
                Ok::<(), std::io::Error>(())
            }
        } => Ok(()),
    };

    if let Err(err) = shutdown_result {
        error!(error = %err, "Failed to setup shutdown signal handlers");
    }

    shutdown_token.cancel();

    let shutdown_timeout = Duration::from_secs(30);
    match tokio::time::timeout(shutdown_timeout, async {
        for task in sync_tasks {
            if let Err(err) = task.await {
                warn!(error = %err, "Channel worker panicked during shutdown");
            }
        }

        if let Err(err) = dispatcher_task.await {
            warn!(error = %err, "Dispatcher task panicked during shutdown");
        }

        connection.shutdown().await;
    })
    .await
    {
        Ok(_) => info!("Telegram indexer shutdown completed"),
        Err(_) => warn!("Shutdown timeout exceeded; forcing process exit"),
    }

    Ok(())
}

async fn connect_db_with_retry(
    database_config: &common::config::DatabaseConfig,
) -> Arc<sea_orm::DatabaseConnection> {
    let mut attempt = 1u32;

    loop {
        match db_core::connection::from_config(database_config).await {
            Ok(db) => {
                info!(attempt, "Database connection established");
                return Arc::new(db);
            }
            Err(err) => {
                warn!(
                    attempt,
                    error = %err,
                    retry_seconds = 5,
                    "Database connect failed; retrying"
                );
                attempt = attempt.saturating_add(1);
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}
