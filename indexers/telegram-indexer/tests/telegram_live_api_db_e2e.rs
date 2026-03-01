#![cfg(feature = "live_telegram_tests")]

mod support;

use anyhow::{Context, Result};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use std::sync::Arc;
use std::time::Duration;
use support::{init_test_tracing, TestDatabase};
use telegram_indexer::sync::MessageProcessor;
use telegram_indexer::{ChannelManager, RuntimeConfig, TelegramAuth, TelegramClient};

use entities::telegram::prelude::TelegramMessages;
use entities::telegram::telegram_messages;

#[tokio::test]
#[ignore = "hits real Telegram API"]
async fn telegram_live_api_ingests_recent_messages_into_postgres() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();

    let runtime_config = Arc::new(runtime_config_from_env()?);
    let connection = TelegramAuth::connect(runtime_config.as_ref())
        .await
        .context("failed to connect authorized Telegram session")?;

    let run_result = async {
        let telegram_client = Arc::new(TelegramClient::new(
            connection.client.clone(),
            runtime_config.clone(),
        ));

        if !telegram_client.is_connected().await {
            anyhow::bail!("Telegram client is not authorized");
        }

        let channel_manager = ChannelManager::new(telegram_client.clone(), conn.clone());
        let first_channel = runtime_config
            .channels
            .first()
            .cloned()
            .context("TELEGRAM_CHANNELS must include at least one channel")?;

        let channels = channel_manager
            .resolve_and_ensure_channels(std::slice::from_ref(&first_channel))
            .await
            .context("failed to resolve live Telegram channel")?;

        let (channel_db_id, chat) = channels
            .into_iter()
            .next()
            .context("no channel resolved from TELEGRAM_CHANNELS")?;

        let chat_ref = chat
            .to_ref()
            .await
            .context("failed to get peer ref for live channel")?;

        let message_limit = std::env::var("TELEGRAM_LIVE_TEST_MESSAGE_LIMIT")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(5)
            .max(1);

        let mut iter = telegram_client
            .client()
            .iter_messages(chat_ref)
            .limit(message_limit);
        let processor = MessageProcessor::new(conn.clone());

        let mut processed = 0u64;
        while processed < message_limit as u64 {
            let next = tokio::time::timeout(Duration::from_secs(45), iter.next())
                .await
                .context("timed out waiting for Telegram history")?;

            let Some(message) = next? else {
                break;
            };

            processor
                .process_message(&message, channel_db_id)
                .await
                .context("failed to persist live Telegram message")?;

            processed = processed.saturating_add(1);
        }

        assert!(processed > 0, "expected at least one live Telegram message");

        let stored_count = TelegramMessages::find()
            .filter(telegram_messages::Column::ChannelId.eq(channel_db_id))
            .count(conn.as_ref())
            .await?;

        assert!(
            stored_count > 0,
            "expected Telegram messages to be persisted in Postgres"
        );

        Ok(())
    }
    .await;

    connection.shutdown().await;
    run_result
}

fn runtime_config_from_env() -> Result<RuntimeConfig> {
    let api_id = required_env("TELEGRAM_API_ID")?
        .parse::<i32>()
        .context("TELEGRAM_API_ID must be a valid integer")?;
    let api_hash = required_env("TELEGRAM_API_HASH")?;
    let channels_raw = required_env("TELEGRAM_CHANNELS")?;

    let channels = channels_raw
        .split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect::<Vec<_>>();

    if channels.is_empty() {
        anyhow::bail!("TELEGRAM_CHANNELS must contain at least one channel");
    }

    let session_file = std::env::var("TELEGRAM_SESSION_FILE")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "telegram_session.bin".to_string());

    let session_data = std::env::var("TELEGRAM_SESSION_DATA")
        .ok()
        .filter(|value| !value.trim().is_empty());

    let sync_batch_size = std::env::var("SYNC_BATCH_SIZE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100);

    let max_concurrent_channels = std::env::var("MAX_CONCURRENT_CHANNELS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(5);

    let max_retry_attempts = std::env::var("MAX_RETRY_ATTEMPTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(3);

    let retry_base_delay_seconds = std::env::var("RETRY_BASE_DELAY_SECONDS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(60);

    Ok(RuntimeConfig {
        api_id,
        api_hash,
        session_file,
        session_data,
        channels,
        sync_batch_size,
        max_concurrent_channels,
        max_retry_attempts,
        retry_base_delay_seconds,
        max_messages_to_sync: None,
        force_full_backfill: false,
        allow_interactive_login: false,
    })
}

fn required_env(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("{name} environment variable is required"))
}
