use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use grammers_client::media::Media;
use grammers_client::message::Message;
use grammers_client::peer::Peer as ClientPeer;
use grammers_tl_types as tl;
use grammers_tl_types::enums::{MessageAction, MessageReactions, Peer, Reaction};
use sea_orm::sea_query::{Expr, OnConflict};
use sea_orm::{
    ActiveValue::NotSet, ColumnTrait, DatabaseTransaction, EntityTrait, QueryFilter, Set, SqlErr,
    TransactionTrait,
};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, warn};

use entities::telegram::prelude::{
    TelegramMessages, TelegramReactions, TelegramServiceActions, TelegramUsers,
};
use entities::telegram::{
    telegram_messages, telegram_reactions, telegram_service_actions, telegram_users,
};

const DB_MEDIA_TYPE_LIMIT: usize = 64;

#[derive(Debug, Clone)]
struct ReactionSnapshot {
    peer: Peer,
    emoji: String,
    timestamp: i32,
}

#[derive(Debug)]
enum ReactionSyncState {
    NotAvailable,
    Clear,
    Records(Vec<ReactionSnapshot>),
}

pub struct MessageProcessor {
    db: Arc<sea_orm::DatabaseConnection>,
}

impl MessageProcessor {
    pub fn new(db: Arc<sea_orm::DatabaseConnection>) -> Self {
        Self { db }
    }

    pub async fn process_message(&self, message: &Message, channel_id: i64) -> Result<()> {
        let telegram_message_id = i64::from(message.id());
        debug!(
            channel_id,
            telegram_message_id, "Processing Telegram message"
        );

        let txn = self
            .db
            .begin()
            .await
            .context("Failed to start transaction for telegram message")?;

        match self
            .process_message_in_transaction(&txn, message, channel_id)
            .await
        {
            Ok(()) => {
                txn.commit().await.context("Failed to commit transaction")?;
                Ok(())
            }
            Err(err) => {
                if let Err(rollback_err) = txn.rollback().await {
                    warn!(error = %rollback_err, "Failed to rollback Telegram message transaction");
                }
                Err(err)
            }
        }
    }

    async fn process_message_in_transaction(
        &self,
        txn: &DatabaseTransaction,
        message: &Message,
        channel_id: i64,
    ) -> Result<()> {
        let telegram_message_id = i64::from(message.id());

        let from_user_id = if let Some(sender) = message.sender() {
            Some(self.process_user(txn, sender).await?)
        } else {
            None
        };

        let text = if message.text().is_empty() {
            None
        } else {
            Some(message.text().to_string())
        };

        let message_type = self.determine_message_type(message);
        let text_entities = self.extract_text_entities(message);
        let (has_media, media_type, media_path) = self.extract_media_info(message);

        let raw_reply_id = message.reply_to_message_id().map(i64::from);
        let (reply_to_message_id, reply_to_telegram_message_id) =
            if let Some(reply_telegram_id) = raw_reply_id {
                (
                    self.find_reply_message_id(txn, channel_id, reply_telegram_id)
                        .await?,
                    Some(reply_telegram_id),
                )
            } else {
                (None, None)
            };

        let db_message_id = self
            .create_message_record(
                txn,
                channel_id,
                telegram_message_id,
                &message_type,
                from_user_id,
                reply_to_message_id,
                reply_to_telegram_message_id,
                message.date().naive_utc(),
                message.edit_date().map(|value| value.naive_utc()),
                text,
                text_entities,
                has_media,
                media_type,
                media_path,
            )
            .await?;

        self.resolve_pending_replies(txn, channel_id, telegram_message_id, db_message_id)
            .await?;

        self.sync_message_reactions(txn, message, db_message_id)
            .await?;

        if let Some(action) = message.action() {
            self.process_service_action(txn, db_message_id, action, from_user_id)
                .await?;
        }

        Ok(())
    }

    async fn process_user(&self, txn: &DatabaseTransaction, chat: &ClientPeer) -> Result<i64> {
        let telegram_id = chat.id().bot_api_dialog_id();
        let username = chat.username().map(|value| value.to_string());
        let display_name = chat.name().map(|value| value.to_string());
        let now = Utc::now().naive_utc();

        let model = telegram_users::ActiveModel {
            id: NotSet,
            telegram_id: Set(telegram_id),
            username: Set(username.clone()),
            display_name: Set(display_name),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let user = match TelegramUsers::insert(model)
            .on_conflict(
                OnConflict::column(telegram_users::Column::TelegramId)
                    .update_columns([
                        telegram_users::Column::Username,
                        telegram_users::Column::DisplayName,
                        telegram_users::Column::UpdatedAt,
                    ])
                    .to_owned(),
            )
            .exec_with_returning(txn)
            .await
        {
            Ok(user) => user,
            Err(db_err) => {
                if matches!(db_err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
                    TelegramUsers::find()
                        .filter(telegram_users::Column::TelegramId.eq(telegram_id))
                        .one(txn)
                        .await
                        .context("Failed to load existing telegram user after conflict")?
                        .context("Telegram user missing after unique conflict")?
                } else {
                    return Err(db_err).context("Failed to upsert telegram user");
                }
            }
        };

        debug!(
            telegram_id,
            user_id = user.id,
            ?username,
            "Upserted Telegram user"
        );
        Ok(user.id)
    }

    fn determine_message_type(&self, message: &Message) -> String {
        if message.action().is_some() {
            "service".to_string()
        } else if message.media().is_some() {
            "media".to_string()
        } else if !message.text().is_empty() {
            "message".to_string()
        } else {
            "unknown".to_string()
        }
    }

    fn extract_text_entities(&self, message: &Message) -> Option<JsonValue> {
        let entities = message.fmt_entities()?;
        if entities.is_empty() {
            return None;
        }

        let payload: Vec<JsonValue> = entities
            .iter()
            .map(|entity| {
                let entity_type = match entity {
                    grammers_tl_types::enums::MessageEntity::Bold(_) => "bold",
                    grammers_tl_types::enums::MessageEntity::Italic(_) => "italic",
                    grammers_tl_types::enums::MessageEntity::Code(_) => "code",
                    grammers_tl_types::enums::MessageEntity::Pre(_) => "pre",
                    grammers_tl_types::enums::MessageEntity::Mention(_) => "mention",
                    grammers_tl_types::enums::MessageEntity::Hashtag(_) => "hashtag",
                    grammers_tl_types::enums::MessageEntity::BotCommand(_) => "bot_command",
                    grammers_tl_types::enums::MessageEntity::Url(_) => "url",
                    grammers_tl_types::enums::MessageEntity::Email(_) => "email",
                    grammers_tl_types::enums::MessageEntity::TextUrl(_) => "text_url",
                    grammers_tl_types::enums::MessageEntity::MentionName(_) => "mention_name",
                    grammers_tl_types::enums::MessageEntity::Underline(_) => "underline",
                    grammers_tl_types::enums::MessageEntity::Strike(_) => "strikethrough",
                    grammers_tl_types::enums::MessageEntity::Spoiler(_) => "spoiler",
                    grammers_tl_types::enums::MessageEntity::CustomEmoji(_) => "custom_emoji",
                    _ => "unknown",
                };

                json!({
                    "type": entity_type,
                    "offset": entity.offset(),
                    "length": entity.length(),
                })
            })
            .collect();

        Some(json!(payload))
    }

    fn extract_media_info(&self, message: &Message) -> (bool, Option<String>, Option<String>) {
        if let Some(media) = message.media() {
            let media_type = Self::normalize_media_type(Self::summarize_media_kind(&media));
            (true, Some(media_type), None)
        } else {
            (false, None, None)
        }
    }

    fn summarize_media_kind(media: &Media) -> String {
        use Media::*;

        match media {
            Photo(_) => "photo".to_string(),
            Document(document) => {
                if let Some(mime) = document.mime_type() {
                    if mime.starts_with("video/") {
                        "document.video".to_string()
                    } else if mime.starts_with("audio/") {
                        "document.audio".to_string()
                    } else if mime == "application/pdf" {
                        "document.pdf".to_string()
                    } else if mime == "image/gif" {
                        "document.gif".to_string()
                    } else {
                        format!("document:{mime}")
                    }
                } else if document.is_animated() {
                    "document.animated".to_string()
                } else {
                    "document".to_string()
                }
            }
            Sticker(sticker) => {
                if sticker.is_animated() {
                    "sticker.animated".to_string()
                } else {
                    "sticker".to_string()
                }
            }
            Contact(_) => "contact".to_string(),
            Poll(_) => "poll".to_string(),
            Geo(_) => "geo".to_string(),
            Dice(_) => "dice".to_string(),
            Venue(_) => "venue".to_string(),
            GeoLive(_) => "geolive".to_string(),
            WebPage(_) => "web_page".to_string(),
            _ => "unknown".to_string(),
        }
    }

    fn normalize_media_type(mut media_type: String) -> String {
        if media_type.len() > DB_MEDIA_TYPE_LIMIT {
            media_type.truncate(DB_MEDIA_TYPE_LIMIT);
        }
        media_type
    }

    async fn find_reply_message_id(
        &self,
        txn: &DatabaseTransaction,
        channel_id: i64,
        reply_telegram_message_id: i64,
    ) -> Result<Option<i64>> {
        let existing = TelegramMessages::find()
            .filter(telegram_messages::Column::ChannelId.eq(channel_id))
            .filter(telegram_messages::Column::TelegramMessageId.eq(reply_telegram_message_id))
            .one(txn)
            .await
            .context("Failed to query reply target message")?;

        Ok(existing.map(|row| row.id))
    }

    #[allow(clippy::too_many_arguments)]
    async fn create_message_record(
        &self,
        txn: &DatabaseTransaction,
        channel_id: i64,
        telegram_message_id: i64,
        message_type: &str,
        from_user_id: Option<i64>,
        reply_to_message_id: Option<i64>,
        reply_to_telegram_message_id: Option<i64>,
        date: chrono::NaiveDateTime,
        edited_date: Option<chrono::NaiveDateTime>,
        text: Option<String>,
        text_entities: Option<JsonValue>,
        has_media: bool,
        media_type: Option<String>,
        media_path: Option<String>,
    ) -> Result<i64> {
        let now = Utc::now().naive_utc();

        let model = telegram_messages::ActiveModel {
            id: NotSet,
            channel_id: Set(channel_id),
            telegram_message_id: Set(telegram_message_id),
            message_type: Set(message_type.to_string()),
            from_user_id: Set(from_user_id),
            reply_to_message_id: Set(reply_to_message_id),
            reply_to_telegram_message_id: Set(reply_to_telegram_message_id),
            date: Set(date),
            edited_date: Set(edited_date),
            text: Set(text),
            text_entities: Set(text_entities),
            has_media: Set(has_media),
            media_type: Set(media_type),
            media_path: Set(media_path),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let row = TelegramMessages::insert(model)
            .on_conflict(
                OnConflict::columns([
                    telegram_messages::Column::ChannelId,
                    telegram_messages::Column::TelegramMessageId,
                ])
                .update_columns([
                    telegram_messages::Column::MessageType,
                    telegram_messages::Column::FromUserId,
                    telegram_messages::Column::ReplyToMessageId,
                    telegram_messages::Column::ReplyToTelegramMessageId,
                    telegram_messages::Column::EditedDate,
                    telegram_messages::Column::Text,
                    telegram_messages::Column::TextEntities,
                    telegram_messages::Column::HasMedia,
                    telegram_messages::Column::MediaType,
                    telegram_messages::Column::MediaPath,
                    telegram_messages::Column::UpdatedAt,
                ])
                .to_owned(),
            )
            .exec_with_returning(txn)
            .await
            .with_context(|| {
                format!(
                    "Failed to upsert telegram message {} in channel {}",
                    telegram_message_id, channel_id
                )
            })?;

        Ok(row.id)
    }

    async fn resolve_pending_replies(
        &self,
        txn: &DatabaseTransaction,
        channel_id: i64,
        telegram_message_id: i64,
        message_id: i64,
    ) -> Result<()> {
        let now = Utc::now().naive_utc();
        let result = TelegramMessages::update_many()
            .col_expr(
                telegram_messages::Column::ReplyToMessageId,
                Expr::value(message_id),
            )
            .col_expr(telegram_messages::Column::UpdatedAt, Expr::value(now))
            .filter(telegram_messages::Column::ChannelId.eq(channel_id))
            .filter(telegram_messages::Column::ReplyToTelegramMessageId.eq(telegram_message_id))
            .filter(telegram_messages::Column::ReplyToMessageId.is_null())
            .exec(txn)
            .await
            .context("Failed to backfill pending reply relationships")?;

        if result.rows_affected > 0 {
            debug!(
                channel_id,
                telegram_message_id,
                rows_affected = result.rows_affected,
                "Resolved pending reply links"
            );
        }

        Ok(())
    }

    async fn sync_message_reactions(
        &self,
        txn: &DatabaseTransaction,
        message: &Message,
        message_id: i64,
    ) -> Result<()> {
        match Self::extract_reaction_state(message) {
            ReactionSyncState::NotAvailable => Ok(()),
            ReactionSyncState::Clear => {
                TelegramReactions::delete_many()
                    .filter(telegram_reactions::Column::MessageId.eq(message_id))
                    .exec(txn)
                    .await
                    .context("Failed to clear message reactions")?;
                Ok(())
            }
            ReactionSyncState::Records(records) => {
                TelegramReactions::delete_many()
                    .filter(telegram_reactions::Column::MessageId.eq(message_id))
                    .exec(txn)
                    .await
                    .context("Failed to reset existing message reactions")?;

                let mut user_cache: HashMap<i64, i64> = HashMap::new();
                let mut aggregated: HashMap<(i64, String), chrono::NaiveDateTime> = HashMap::new();

                for snapshot in records {
                    if let Some(user_telegram_id) = Self::peer_to_user_id(&snapshot.peer) {
                        let reaction_at = Self::timestamp_to_naive(snapshot.timestamp);
                        let key = (user_telegram_id, snapshot.emoji);

                        aggregated
                            .entry(key)
                            .and_modify(|existing| {
                                if reaction_at > *existing {
                                    *existing = reaction_at;
                                }
                            })
                            .or_insert(reaction_at);
                    }
                }

                for ((telegram_user_id, emoji), reacted_at) in aggregated {
                    let user_id = if let Some(cached) = user_cache.get(&telegram_user_id) {
                        *cached
                    } else {
                        let resolved = self.ensure_user_stub(txn, telegram_user_id).await?;
                        user_cache.insert(telegram_user_id, resolved);
                        resolved
                    };

                    let model = telegram_reactions::ActiveModel {
                        id: NotSet,
                        message_id: Set(message_id),
                        user_id: Set(user_id),
                        emoji: Set(emoji),
                        date: Set(reacted_at),
                        created_at: Set(Utc::now().naive_utc()),
                    };

                    TelegramReactions::insert(model)
                        .exec(txn)
                        .await
                        .context("Failed to insert telegram reaction")?;
                }

                Ok(())
            }
        }
    }

    fn extract_reaction_state(message: &Message) -> ReactionSyncState {
        match &message.raw {
            tl::enums::Message::Message(inner) => match &inner.reactions {
                None => ReactionSyncState::Clear,
                Some(MessageReactions::Reactions(reactions)) => {
                    if reactions.min || !reactions.can_see_list {
                        return ReactionSyncState::NotAvailable;
                    }

                    let mut snapshots = Vec::new();
                    if let Some(recent) = &reactions.recent_reactions {
                        for item in recent {
                            let tl::enums::MessagePeerReaction::Reaction(reaction) = item;
                            if let Some(emoji) = Self::reaction_to_emoji(&reaction.reaction) {
                                snapshots.push(ReactionSnapshot {
                                    peer: reaction.peer_id.clone(),
                                    emoji,
                                    timestamp: reaction.date,
                                });
                            }
                        }
                    }

                    if snapshots.is_empty() {
                        ReactionSyncState::Clear
                    } else {
                        ReactionSyncState::Records(snapshots)
                    }
                }
            },
            _ => ReactionSyncState::NotAvailable,
        }
    }

    fn reaction_to_emoji(reaction: &Reaction) -> Option<String> {
        match reaction {
            Reaction::Emoji(value) => Some(value.emoticon.clone()),
            Reaction::CustomEmoji(value) => Some(format!("custom:{}", value.document_id)),
            Reaction::Paid => Some("paid".to_string()),
            Reaction::Empty => None,
        }
    }

    fn peer_to_user_id(peer: &Peer) -> Option<i64> {
        match peer {
            Peer::User(user) => Some(user.user_id),
            _ => None,
        }
    }

    async fn ensure_user_stub(&self, txn: &DatabaseTransaction, telegram_id: i64) -> Result<i64> {
        let now = Utc::now().naive_utc();
        let model = telegram_users::ActiveModel {
            id: NotSet,
            telegram_id: Set(telegram_id),
            username: Set(None),
            display_name: Set(None),
            created_at: Set(now),
            updated_at: Set(now),
        };

        let user = match TelegramUsers::insert(model)
            .on_conflict(
                OnConflict::column(telegram_users::Column::TelegramId)
                    .update_columns([telegram_users::Column::UpdatedAt])
                    .to_owned(),
            )
            .exec_with_returning(txn)
            .await
        {
            Ok(user) => user,
            Err(db_err) => {
                if matches!(db_err.sql_err(), Some(SqlErr::UniqueConstraintViolation(_))) {
                    TelegramUsers::find()
                        .filter(telegram_users::Column::TelegramId.eq(telegram_id))
                        .one(txn)
                        .await
                        .context("Failed to load reaction user after conflict")?
                        .context("Reaction user missing after conflict")?
                } else {
                    return Err(db_err).context("Failed to upsert reaction user");
                }
            }
        };

        Ok(user.id)
    }

    fn timestamp_to_naive(timestamp: i32) -> chrono::NaiveDateTime {
        DateTime::<Utc>::from_timestamp(i64::from(timestamp), 0)
            .unwrap_or_else(Utc::now)
            .naive_utc()
    }

    async fn process_service_action(
        &self,
        txn: &DatabaseTransaction,
        message_id: i64,
        action: &MessageAction,
        actor_id: Option<i64>,
    ) -> Result<()> {
        let raw_action = format!("{action:?}");
        let metadata = json!({
            "raw_action": raw_action,
            "processed_at": Utc::now().to_rfc3339(),
        });

        let model = telegram_service_actions::ActiveModel {
            id: NotSet,
            message_id: Set(message_id),
            action: Set(self.normalize_action_type(&raw_action)),
            actor_id: Set(actor_id),
            metadata: Set(Some(metadata)),
            created_at: Set(Utc::now().naive_utc()),
        };

        TelegramServiceActions::insert(model)
            .on_conflict(
                OnConflict::columns([
                    telegram_service_actions::Column::MessageId,
                    telegram_service_actions::Column::Action,
                ])
                .update_columns([
                    telegram_service_actions::Column::ActorId,
                    telegram_service_actions::Column::Metadata,
                ])
                .to_owned(),
            )
            .exec(txn)
            .await
            .context("Failed to upsert telegram service action")?;

        Ok(())
    }

    fn normalize_action_type(&self, raw_action: &str) -> String {
        if raw_action.contains("ChatAddUser") {
            "user_joined".to_string()
        } else if raw_action.contains("ChatDeleteUser") {
            "user_left".to_string()
        } else if raw_action.contains("ChatEditTitle") {
            "title_changed".to_string()
        } else if raw_action.contains("ChatEditPhoto") {
            "photo_changed".to_string()
        } else if raw_action.contains("ChatDeletePhoto") {
            "photo_deleted".to_string()
        } else if raw_action.contains("PinMessage") {
            "message_pinned".to_string()
        } else if raw_action.contains("UnpinMessage") {
            "message_unpinned".to_string()
        } else {
            raw_action
                .chars()
                .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
                .collect::<String>()
                .to_lowercase()
        }
    }
}
