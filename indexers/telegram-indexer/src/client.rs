use anyhow::{Context, Result};
use grammers_client::{peer::Peer, Client};
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::config::RuntimeConfig;

pub struct TelegramClient {
    client: Client,
    config: Arc<RuntimeConfig>,
}

impl TelegramClient {
    pub fn new(client: Client, config: Arc<RuntimeConfig>) -> Self {
        Self { client, config }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }

    pub fn config(&self) -> &RuntimeConfig {
        &self.config
    }

    pub async fn resolve_channel(&self, channel_identifier: &str) -> Result<Peer> {
        debug!(channel_identifier, "Resolving Telegram channel");

        if channel_identifier.starts_with('-')
            || channel_identifier.chars().all(|ch| ch.is_ascii_digit())
        {
            self.find_channel_by_id(channel_identifier).await
        } else {
            let username = channel_identifier
                .strip_prefix('@')
                .unwrap_or(channel_identifier);

            match self
                .client
                .resolve_username(username)
                .await
                .with_context(|| format!("Failed resolving channel username {}", username))?
            {
                Some(chat) => {
                    info!(
                        channel_identifier,
                        resolved_name = %chat.name().unwrap_or("unknown"),
                        "Resolved Telegram channel by username"
                    );
                    Ok(chat)
                }
                None => Err(anyhow::anyhow!(
                    "Telegram channel not found for identifier {}",
                    channel_identifier
                )),
            }
        }
    }

    async fn find_channel_by_id(&self, channel_id_raw: &str) -> Result<Peer> {
        let target_id: i64 = channel_id_raw
            .parse()
            .with_context(|| format!("Invalid Telegram channel id {}", channel_id_raw))?;

        let possible_ids = Self::possible_channel_ids(target_id);
        info!(target_id, ?possible_ids, "Searching dialogs for channel id");

        let mut dialogs = self.client.iter_dialogs();
        while let Some(dialog) = dialogs
            .next()
            .await
            .context("Failed iterating Telegram dialogs")?
        {
            let chat = dialog.peer();
            let chat_id = chat.id().bot_api_dialog_id();

            if possible_ids.contains(&chat_id) {
                info!(
                    target_id,
                    matched_id = chat_id,
                    channel_name = %chat.name().unwrap_or("unknown"),
                    "Matched Telegram channel id from dialogs"
                );
                return Ok(chat.clone());
            }
        }

        Err(anyhow::anyhow!(
            "Telegram channel id {} not found in dialogs (tried {:?})",
            target_id,
            possible_ids
        ))
    }

    fn possible_channel_ids(id: i64) -> Vec<i64> {
        const CHANNEL_PREFIX: i64 = 1_000_000_000_000;

        let mut ids = vec![id, -id, id.abs()];
        let abs_id = id.abs();

        if abs_id > CHANNEL_PREFIX {
            ids.push(abs_id - CHANNEL_PREFIX);
            ids.push(-(abs_id - CHANNEL_PREFIX));
        } else if abs_id > 0 {
            ids.push(CHANNEL_PREFIX + abs_id);
            ids.push(-(CHANNEL_PREFIX + abs_id));
        }

        ids.sort_unstable();
        ids.dedup();
        ids
    }

    pub async fn is_connected(&self) -> bool {
        match self.client.is_authorized().await {
            Ok(authorized) => authorized,
            Err(err) => {
                error!(error = %err, "Failed checking Telegram authorization status");
                false
            }
        }
    }

    pub async fn get_channel_info(&self, chat: &Peer) -> Result<ChannelInfo> {
        Ok(ChannelInfo {
            id: chat.id().bot_api_dialog_id(),
            name: chat.name().unwrap_or("unknown").to_string(),
            username: chat.username().map(|value| value.to_string()),
            chat_type: match chat {
                Peer::User(_) => "user".to_string(),
                Peer::Group(_) => "group".to_string(),
                Peer::Channel(_) => "channel".to_string(),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::TelegramClient;

    #[test]
    fn expands_plain_channel_id_to_prefixed_forms() {
        let ids = TelegramClient::possible_channel_ids(3_047_964_827);
        assert!(ids.contains(&3_047_964_827));
        assert!(ids.contains(&-3_047_964_827));
        assert!(ids.contains(&-1_003_047_964_827));
    }

    #[test]
    fn normalizes_prefixed_negative_channel_id() {
        let ids = TelegramClient::possible_channel_ids(-1_003_047_964_827);
        assert!(ids.contains(&-1_003_047_964_827));
        assert!(ids.contains(&3_047_964_827));
    }
}

#[derive(Debug, Clone)]
pub struct ChannelInfo {
    pub id: i64,
    pub name: String,
    pub username: Option<String>,
    pub chat_type: String,
}
