use anyhow::{Context, Result};
use grammers_client::peer::Peer;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveValue::NotSet, EntityTrait, Set};
use std::sync::Arc;
use tracing::{debug, info};

use crate::client::{ChannelInfo, TelegramClient};
use entities::telegram::telegram_channels;
use entities::telegram::telegram_channels::Entity as TelegramChannels;

pub struct ChannelManager {
    client: Arc<TelegramClient>,
    db: Arc<sea_orm::DatabaseConnection>,
}

impl ChannelManager {
    pub fn new(client: Arc<TelegramClient>, db: Arc<sea_orm::DatabaseConnection>) -> Self {
        Self { client, db }
    }

    pub async fn resolve_and_ensure_channels(
        &self,
        channel_identifiers: &[String],
    ) -> Result<Vec<(i64, Peer)>> {
        let mut channels = Vec::new();

        for identifier in channel_identifiers {
            info!(identifier, "Resolving Telegram channel");

            let chat = self
                .client
                .resolve_channel(identifier)
                .await
                .with_context(|| format!("Failed resolving Telegram channel {}", identifier))?;

            let channel_info = self
                .client
                .get_channel_info(&chat)
                .await
                .context("Failed loading Telegram channel metadata")?;

            let channel_id = self.ensure_channel(&channel_info).await?;
            channels.push((channel_id, chat));
        }

        Ok(channels)
    }

    async fn ensure_channel(&self, channel: &ChannelInfo) -> Result<i64> {
        debug!(telegram_id = channel.id, name = %channel.name, "Ensuring channel exists");

        let now = chrono::Utc::now().naive_utc();
        let active_model = telegram_channels::ActiveModel {
            id: NotSet,
            telegram_id: Set(channel.id),
            name: Set(channel.name.clone()),
            username: Set(channel.username.clone()),
            r#type: Set(channel.chat_type.clone()),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let model = TelegramChannels::insert(active_model)
            .on_conflict(
                OnConflict::column(telegram_channels::Column::TelegramId)
                    .update_columns([
                        telegram_channels::Column::Name,
                        telegram_channels::Column::Username,
                        telegram_channels::Column::Type,
                        telegram_channels::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec_with_returning(self.db.as_ref())
            .await
            .context("Failed to upsert telegram channel")?;

        info!(
            channel_id = model.id,
            telegram_id = model.telegram_id,
            name = %model.name,
            "Upserted Telegram channel"
        );

        Ok(model.id)
    }
}
