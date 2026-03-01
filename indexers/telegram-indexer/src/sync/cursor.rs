use anyhow::{Context, Result};
use grammers_client::peer::Peer;
use grammers_client::update::Update;
use grammers_session::types::PeerRef;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use std::sync::Arc;
use tokio::sync::{mpsc::UnboundedReceiver, OwnedSemaphorePermit};
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};

use crate::{client::TelegramClient, config::RuntimeConfig, sync::MessageProcessor};
use entities::telegram::telegram_messages;
use entities::telegram::telegram_messages::Entity as TelegramMessages;

pub struct ChannelSyncCursor {
    channel_db_id: i64,
    chat: Peer,
    chat_ref: PeerRef,
    historical_stop_at_id: Option<i64>,
    last_message_id: Option<i64>,
    client: Arc<TelegramClient>,
    db: Arc<sea_orm::DatabaseConnection>,
    config: Arc<RuntimeConfig>,
    updates_rx: UnboundedReceiver<Update>,
}

impl ChannelSyncCursor {
    pub async fn load_or_create(
        channel_db_id: i64,
        chat: Peer,
        client: Arc<TelegramClient>,
        db: Arc<sea_orm::DatabaseConnection>,
        config: Arc<RuntimeConfig>,
        updates_rx: UnboundedReceiver<Update>,
    ) -> Result<Self> {
        let persisted_last_message_id =
            Self::fetch_last_synced_message_id(db.as_ref(), channel_db_id)
                .await
                .context("Failed to fetch latest synced telegram message id")?;

        let historical_stop_at_id = if config.force_full_backfill {
            None
        } else {
            persisted_last_message_id
        };

        match (persisted_last_message_id, config.force_full_backfill) {
            (Some(message_id), false) => {
                info!(
                    channel = %chat.name().unwrap_or("unknown"),
                    message_id,
                    "Resuming Telegram channel sync"
                );
            }
            (Some(message_id), true) => {
                info!(
                    channel = %chat.name().unwrap_or("unknown"),
                    message_id,
                    "TELEGRAM_FORCE_FULL_BACKFILL is enabled; ignoring sync boundary"
                );
            }
            (None, _) => {
                info!(
                    channel = %chat.name().unwrap_or("unknown"),
                    "No existing Telegram messages found, running full historical import"
                );
            }
        }

        let chat_ref = chat
            .to_ref()
            .await
            .context("Failed to resolve Telegram peer reference for channel")?;

        Ok(Self {
            channel_db_id,
            chat,
            chat_ref,
            historical_stop_at_id,
            last_message_id: persisted_last_message_id,
            client,
            db,
            config,
            updates_rx,
        })
    }

    pub async fn run_sync(&mut self, concurrency_guard: OwnedSemaphorePermit) -> Result<()> {
        let processor = MessageProcessor::new(self.db.clone());

        self.sync_historical_messages(&processor).await?;

        drop(concurrency_guard);

        info!(
            channel = %self.chat.name().unwrap_or("unknown"),
            "Historical sync complete; entering realtime mode"
        );

        self.monitor_real_time_updates(&processor).await
    }

    async fn fetch_last_synced_message_id(
        db: &sea_orm::DatabaseConnection,
        channel_db_id: i64,
    ) -> Result<Option<i64>> {
        let latest = TelegramMessages::find()
            .filter(telegram_messages::Column::ChannelId.eq(channel_db_id))
            .order_by_desc(telegram_messages::Column::TelegramMessageId)
            .one(db)
            .await?;

        Ok(latest.map(|row| row.telegram_message_id))
    }

    async fn sync_historical_messages(&mut self, processor: &MessageProcessor) -> Result<()> {
        let channel_name = self.chat.name().unwrap_or("unknown").to_string();

        let mut messages = self.client.client().iter_messages(self.chat_ref);

        if let Some(limit) = self.config.max_messages_to_sync {
            info!(channel = %channel_name, limit, "Applying historical message sync limit");
            messages = messages.limit(limit);
        }

        let mut processed_count: u64 = 0;
        while let Some(message) = messages.next().await? {
            let message_id = i64::from(message.id());

            if Self::reached_historical_boundary(message_id, self.historical_stop_at_id) {
                debug!(
                    channel = %channel_name,
                    message_id,
                    boundary = self.historical_stop_at_id,
                    "Reached already-synced Telegram history boundary"
                );
                break;
            }

            if let Err(err) = processor
                .process_message(&message, self.channel_db_id)
                .await
            {
                warn!(
                    channel = %channel_name,
                    message_id,
                    error = %err,
                    "Failed to process historical message; continuing"
                );
                continue;
            }

            self.last_message_id = Some(
                self.last_message_id
                    .map(|current| current.max(message_id))
                    .unwrap_or(message_id),
            );
            processed_count = processed_count.saturating_add(1);

            if processed_count % 100 == 0 {
                info!(
                    channel = %channel_name,
                    processed_count,
                    current_message_id = message_id,
                    "Historical telegram sync progress"
                );
            }

            if processed_count % 10 == 0 {
                sleep(Duration::from_millis(50)).await;
            }
        }

        info!(
            channel = %channel_name,
            processed_count,
            "Historical telegram sync completed"
        );

        Ok(())
    }

    fn reached_historical_boundary(message_id: i64, boundary: Option<i64>) -> bool {
        boundary.is_some_and(|value| message_id <= value)
    }

    async fn monitor_real_time_updates(&mut self, processor: &MessageProcessor) -> Result<()> {
        let channel_name = self.chat.name().unwrap_or("unknown").to_string();
        let channel_telegram_id = self.chat.id().bot_api_dialog_id();

        while let Some(update) = self.updates_rx.recv().await {
            match update {
                Update::NewMessage(message) => {
                    let Some(peer) = message.peer() else {
                        continue;
                    };

                    if peer.id().bot_api_dialog_id() != channel_telegram_id {
                        continue;
                    }

                    let message_id = i64::from(message.id());
                    if let Err(err) = processor
                        .process_message(&message, self.channel_db_id)
                        .await
                    {
                        warn!(
                            channel = %channel_name,
                            message_id,
                            error = %err,
                            "Failed to process realtime Telegram message"
                        );
                        continue;
                    }

                    self.last_message_id = Some(
                        self.last_message_id
                            .map(|current| current.max(message_id))
                            .unwrap_or(message_id),
                    );

                    info!(channel = %channel_name, message_id, "Processed realtime message");
                }
                Update::MessageEdited(message) => {
                    let Some(peer) = message.peer() else {
                        continue;
                    };

                    if peer.id().bot_api_dialog_id() != channel_telegram_id {
                        continue;
                    }

                    let message_id = i64::from(message.id());
                    if let Err(err) = processor
                        .process_message(&message, self.channel_db_id)
                        .await
                    {
                        warn!(
                            channel = %channel_name,
                            message_id,
                            error = %err,
                            "Failed to process realtime Telegram edit"
                        );
                    } else {
                        info!(channel = %channel_name, message_id, "Processed realtime edit");
                    }
                }
                _ => {}
            }
        }

        info!(
            channel = %channel_name,
            "Realtime Telegram update stream closed for channel"
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::ChannelSyncCursor;

    #[test]
    fn historical_boundary_stops_on_existing_id() {
        assert!(ChannelSyncCursor::reached_historical_boundary(
            100,
            Some(100)
        ));
        assert!(ChannelSyncCursor::reached_historical_boundary(
            99,
            Some(100)
        ));
    }

    #[test]
    fn historical_boundary_allows_newer_messages() {
        assert!(!ChannelSyncCursor::reached_historical_boundary(
            101,
            Some(100)
        ));
        assert!(!ChannelSyncCursor::reached_historical_boundary(1, None));
    }
}
