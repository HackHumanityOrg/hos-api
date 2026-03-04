//! Telegram read API handlers.

use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, NaiveDateTime};
use entities::{
    prelude::*,
    telegram::{
        telegram_channels, telegram_messages, telegram_reactions, telegram_service_actions,
        telegram_users,
    },
};
use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Serialize, Deserialize)]
struct TelegramIdCursor {
    telegram_id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct ChannelMessagesCursor {
    date_epoch: i64,
    telegram_message_id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct DateIdCursor {
    date_epoch: i64,
    id: i64,
}

#[utoipa::path(
    get,
    path = "/api/v1/telegram/channels",
    summary = "List indexed Telegram channels",
    description = "Returns indexed Telegram channels known to the Telegram indexer. Supports text search and cursor pagination ordered by Telegram channel id ascending.",
    params(TelegramChannelsQuery),
    tag = "telegram",
    operation_id = "listTelegramChannels",
    responses(
        (status = 200, description = "Telegram channels", body = TelegramChannelsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_channels(
    State(app_state): State<AppState>,
    Query(query): Query<TelegramChannelsQuery>,
) -> Result<Json<TelegramChannelsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.limit);

    let mut builder = TelegramChannels::find().order_by_asc(telegram_channels::Column::TelegramId);

    if let Some(channel_type) = query
        .channel_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        builder = builder.filter(telegram_channels::Column::Type.eq(channel_type));
    }

    if let Some(username) = query
        .username
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        builder = builder.filter(telegram_channels::Column::Username.contains(username));
    }

    if let Some(search) = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        builder = builder.filter(
            Condition::any()
                .add(telegram_channels::Column::Name.contains(search))
                .add(telegram_channels::Column::Username.contains(search)),
        );
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: TelegramIdCursor = decode_cursor(cursor)?;
        builder = builder.filter(telegram_channels::Column::TelegramId.gt(cursor.telegram_id));
    }

    let mut rows = builder
        .limit(limit + 1)
        .all(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?;

    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.pop();
    }

    let next_cursor = rows
        .last()
        .map(|row| {
            encode_cursor(&TelegramIdCursor {
                telegram_id: row.telegram_id,
            })
        })
        .transpose()?;

    Ok(Json(TelegramChannelsListResponse {
        data: rows.into_iter().map(to_channel_dto).collect(),
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/telegram/channels/{channel_telegram_id}",
    summary = "Get one Telegram channel",
    description = "Fetches one indexed Telegram channel by Telegram channel id.",
    params(("channel_telegram_id" = i64, Path, description = "Telegram channel id")),
    tag = "telegram",
    operation_id = "getTelegramChannel",
    responses(
        (status = 200, description = "Telegram channel", body = TelegramChannelDto),
        (status = 404, description = "Channel not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_channel(
    State(app_state): State<AppState>,
    Path(channel_telegram_id): Path<i64>,
) -> Result<Json<TelegramChannelDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let channel = find_channel_by_telegram_id(db.as_ref(), channel_telegram_id).await?;

    Ok(Json(to_channel_dto(channel)))
}

#[utoipa::path(
    get,
    path = "/api/v1/telegram/channels/{channel_telegram_id}/messages",
    summary = "List messages for a channel",
    description = "Returns indexed Telegram messages for a channel ordered by message date descending, then Telegram message id descending. Supports text/media/user filters and cursor pagination.",
    params(
        ("channel_telegram_id" = i64, Path, description = "Telegram channel id"),
        TelegramMessagesQuery
    ),
    tag = "telegram",
    operation_id = "listTelegramChannelMessages",
    responses(
        (status = 200, description = "Telegram messages", body = TelegramMessagesListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Channel not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_channel_messages(
    State(app_state): State<AppState>,
    Path(channel_telegram_id): Path<i64>,
    Query(query): Query<TelegramMessagesQuery>,
) -> Result<Json<TelegramMessagesListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let channel = find_channel_by_telegram_id(db.as_ref(), channel_telegram_id).await?;
    let limit = resolve_limit(query.limit);

    let mut builder = TelegramMessages::find()
        .filter(telegram_messages::Column::ChannelId.eq(channel.id))
        .order_by_desc(telegram_messages::Column::Date)
        .order_by_desc(telegram_messages::Column::TelegramMessageId);

    if let Some(user_telegram_id) = query.user_telegram_id {
        let Some(user) = find_user_by_telegram_id_optional(db.as_ref(), user_telegram_id).await?
        else {
            return Ok(Json(TelegramMessagesListResponse {
                data: Vec::new(),
                next_cursor: None,
                has_more: false,
            }));
        };
        builder = builder.filter(telegram_messages::Column::FromUserId.eq(user.id));
    }

    if let Some(message_type) = query
        .message_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        builder = builder.filter(telegram_messages::Column::MessageType.eq(message_type));
    }

    if let Some(has_media) = query.has_media {
        builder = builder.filter(telegram_messages::Column::HasMedia.eq(has_media));
    }

    if let Some(search) = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        builder = builder.filter(telegram_messages::Column::Text.contains(search));
    }

    if let Some(date_after) = query.date_after.as_deref() {
        let date_after = parse_rfc3339_to_naive(date_after, "date_after")?;
        builder = builder.filter(telegram_messages::Column::Date.gte(date_after));
    }
    if let Some(date_before) = query.date_before.as_deref() {
        let date_before = parse_rfc3339_to_naive(date_before, "date_before")?;
        builder = builder.filter(telegram_messages::Column::Date.lte(date_before));
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: ChannelMessagesCursor = decode_cursor(cursor)?;
        let date = cursor_epoch_to_naive(cursor.date_epoch, "messages cursor")?;
        builder = builder.filter(
            Condition::any()
                .add(telegram_messages::Column::Date.lt(date))
                .add(
                    Condition::all()
                        .add(telegram_messages::Column::Date.eq(date))
                        .add(
                            telegram_messages::Column::TelegramMessageId
                                .lt(cursor.telegram_message_id),
                        ),
                ),
        );
    }

    let mut rows = builder
        .limit(limit + 1)
        .all(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?;

    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.pop();
    }

    let user_ids: Vec<i64> = rows.iter().filter_map(|row| row.from_user_id).collect();
    let users_by_id = users_by_internal_id(db.as_ref(), &user_ids).await?;

    let next_cursor = rows
        .last()
        .map(|row| {
            encode_cursor(&ChannelMessagesCursor {
                date_epoch: naive_to_epoch(row.date),
                telegram_message_id: row.telegram_message_id,
            })
        })
        .transpose()?;

    let data = rows
        .into_iter()
        .map(|row| to_message_dto(row, channel.telegram_id, &users_by_id))
        .collect();

    Ok(Json(TelegramMessagesListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/telegram/channels/{channel_telegram_id}/messages/{telegram_message_id}",
    summary = "Get one channel message",
    description = "Fetches a single indexed Telegram message by channel and Telegram message id.",
    params(
        ("channel_telegram_id" = i64, Path, description = "Telegram channel id"),
        ("telegram_message_id" = i64, Path, description = "Telegram message id")
    ),
    tag = "telegram",
    operation_id = "getTelegramChannelMessage",
    responses(
        (status = 200, description = "Telegram message", body = TelegramMessageDto),
        (status = 404, description = "Message not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_channel_message(
    State(app_state): State<AppState>,
    Path((channel_telegram_id, telegram_message_id)): Path<(i64, i64)>,
) -> Result<Json<TelegramMessageDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let channel = find_channel_by_telegram_id(db.as_ref(), channel_telegram_id).await?;
    let message = find_channel_message(db.as_ref(), channel.id, telegram_message_id).await?;

    let user_ids = message.from_user_id.into_iter().collect::<Vec<_>>();
    let users_by_id = users_by_internal_id(db.as_ref(), &user_ids).await?;

    Ok(Json(to_message_dto(
        message,
        channel.telegram_id,
        &users_by_id,
    )))
}

#[utoipa::path(
    get,
    path = "/api/v1/telegram/channels/{channel_telegram_id}/messages/{telegram_message_id}/reactions",
    summary = "List reactions for a message",
    description = "Returns indexed reactions for one Telegram message ordered by reaction date descending, then row id descending.",
    params(
        ("channel_telegram_id" = i64, Path, description = "Telegram channel id"),
        ("telegram_message_id" = i64, Path, description = "Telegram message id"),
        TelegramReactionsQuery
    ),
    tag = "telegram",
    operation_id = "listTelegramMessageReactions",
    responses(
        (status = 200, description = "Telegram reactions", body = TelegramReactionsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Message not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_message_reactions(
    State(app_state): State<AppState>,
    Path((channel_telegram_id, telegram_message_id)): Path<(i64, i64)>,
    Query(query): Query<TelegramReactionsQuery>,
) -> Result<Json<TelegramReactionsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let channel = find_channel_by_telegram_id(db.as_ref(), channel_telegram_id).await?;
    let message = find_channel_message(db.as_ref(), channel.id, telegram_message_id).await?;
    let limit = resolve_limit(query.limit);

    let mut builder = TelegramReactions::find()
        .filter(telegram_reactions::Column::MessageId.eq(message.id))
        .order_by_desc(telegram_reactions::Column::Date)
        .order_by_desc(telegram_reactions::Column::Id);

    if let Some(emoji) = query
        .emoji
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        builder = builder.filter(telegram_reactions::Column::Emoji.eq(emoji));
    }

    if let Some(user_telegram_id) = query.user_telegram_id {
        let Some(user) = find_user_by_telegram_id_optional(db.as_ref(), user_telegram_id).await?
        else {
            return Ok(Json(TelegramReactionsListResponse {
                data: Vec::new(),
                next_cursor: None,
                has_more: false,
            }));
        };
        builder = builder.filter(telegram_reactions::Column::UserId.eq(user.id));
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: DateIdCursor = decode_cursor(cursor)?;
        let date = cursor_epoch_to_naive(cursor.date_epoch, "reactions cursor")?;
        builder = builder.filter(
            Condition::any()
                .add(telegram_reactions::Column::Date.lt(date))
                .add(
                    Condition::all()
                        .add(telegram_reactions::Column::Date.eq(date))
                        .add(telegram_reactions::Column::Id.lt(cursor.id)),
                ),
        );
    }

    let mut rows = builder
        .limit(limit + 1)
        .all(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?;

    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.pop();
    }

    let user_ids: Vec<i64> = rows.iter().map(|row| row.user_id).collect();
    let users_by_id = users_by_internal_id(db.as_ref(), &user_ids).await?;

    let next_cursor = rows
        .last()
        .map(|row| {
            encode_cursor(&DateIdCursor {
                date_epoch: naive_to_epoch(row.date),
                id: row.id,
            })
        })
        .transpose()?;

    let data = rows
        .into_iter()
        .map(|row| {
            to_reaction_dto(
                row,
                channel.telegram_id,
                message.telegram_message_id,
                &users_by_id,
            )
        })
        .collect();

    Ok(Json(TelegramReactionsListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/telegram/channels/{channel_telegram_id}/messages/{telegram_message_id}/service-actions",
    summary = "List service actions for a message",
    description = "Returns indexed Telegram service-action rows for one message ordered by creation time descending, then row id descending.",
    params(
        ("channel_telegram_id" = i64, Path, description = "Telegram channel id"),
        ("telegram_message_id" = i64, Path, description = "Telegram message id"),
        TelegramServiceActionsQuery
    ),
    tag = "telegram",
    operation_id = "listTelegramMessageServiceActions",
    responses(
        (status = 200, description = "Telegram service actions", body = TelegramServiceActionsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Message not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_message_service_actions(
    State(app_state): State<AppState>,
    Path((channel_telegram_id, telegram_message_id)): Path<(i64, i64)>,
    Query(query): Query<TelegramServiceActionsQuery>,
) -> Result<Json<TelegramServiceActionsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let channel = find_channel_by_telegram_id(db.as_ref(), channel_telegram_id).await?;
    let message = find_channel_message(db.as_ref(), channel.id, telegram_message_id).await?;
    let limit = resolve_limit(query.limit);

    let mut builder = TelegramServiceActions::find()
        .filter(telegram_service_actions::Column::MessageId.eq(message.id))
        .order_by_desc(telegram_service_actions::Column::CreatedAt)
        .order_by_desc(telegram_service_actions::Column::Id);

    if let Some(action) = query
        .action
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        builder = builder.filter(telegram_service_actions::Column::Action.eq(action));
    }

    if let Some(actor_telegram_id) = query.actor_telegram_id {
        let Some(user) = find_user_by_telegram_id_optional(db.as_ref(), actor_telegram_id).await?
        else {
            return Ok(Json(TelegramServiceActionsListResponse {
                data: Vec::new(),
                next_cursor: None,
                has_more: false,
            }));
        };
        builder = builder.filter(telegram_service_actions::Column::ActorId.eq(user.id));
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: DateIdCursor = decode_cursor(cursor)?;
        let created_at = cursor_epoch_to_naive(cursor.date_epoch, "service actions cursor")?;
        builder = builder.filter(
            Condition::any()
                .add(telegram_service_actions::Column::CreatedAt.lt(created_at))
                .add(
                    Condition::all()
                        .add(telegram_service_actions::Column::CreatedAt.eq(created_at))
                        .add(telegram_service_actions::Column::Id.lt(cursor.id)),
                ),
        );
    }

    let mut rows = builder
        .limit(limit + 1)
        .all(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?;

    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.pop();
    }

    let actor_ids: Vec<i64> = rows.iter().filter_map(|row| row.actor_id).collect();
    let users_by_id = users_by_internal_id(db.as_ref(), &actor_ids).await?;

    let next_cursor = rows
        .last()
        .map(|row| {
            encode_cursor(&DateIdCursor {
                date_epoch: naive_to_epoch(row.created_at),
                id: row.id,
            })
        })
        .transpose()?;

    let data = rows
        .into_iter()
        .map(|row| {
            to_service_action_dto(
                row,
                channel.telegram_id,
                message.telegram_message_id,
                &users_by_id,
            )
        })
        .collect();

    Ok(Json(TelegramServiceActionsListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/telegram/users",
    summary = "List indexed Telegram users",
    description = "Returns indexed Telegram users ordered by Telegram user id ascending with optional text search and cursor pagination.",
    params(TelegramUsersQuery),
    tag = "telegram",
    operation_id = "listTelegramUsers",
    responses(
        (status = 200, description = "Telegram users", body = TelegramUsersListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_users(
    State(app_state): State<AppState>,
    Query(query): Query<TelegramUsersQuery>,
) -> Result<Json<TelegramUsersListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.limit);

    let mut builder = TelegramUsers::find().order_by_asc(telegram_users::Column::TelegramId);

    if let Some(search) = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        builder = builder.filter(
            Condition::any()
                .add(telegram_users::Column::Username.contains(search))
                .add(telegram_users::Column::DisplayName.contains(search)),
        );
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: TelegramIdCursor = decode_cursor(cursor)?;
        builder = builder.filter(telegram_users::Column::TelegramId.gt(cursor.telegram_id));
    }

    let mut rows = builder
        .limit(limit + 1)
        .all(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?;

    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.pop();
    }

    let next_cursor = rows
        .last()
        .map(|row| {
            encode_cursor(&TelegramIdCursor {
                telegram_id: row.telegram_id,
            })
        })
        .transpose()?;

    Ok(Json(TelegramUsersListResponse {
        data: rows.into_iter().map(to_user_dto).collect(),
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/telegram/users/{user_telegram_id}",
    summary = "Get one Telegram user",
    description = "Fetches one indexed Telegram user by Telegram user id.",
    params(("user_telegram_id" = i64, Path, description = "Telegram user id")),
    tag = "telegram",
    operation_id = "getTelegramUser",
    responses(
        (status = 200, description = "Telegram user", body = TelegramUserDto),
        (status = 404, description = "User not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_user(
    State(app_state): State<AppState>,
    Path(user_telegram_id): Path<i64>,
) -> Result<Json<TelegramUserDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let user = find_user_by_telegram_id(db.as_ref(), user_telegram_id).await?;

    Ok(Json(to_user_dto(user)))
}

#[utoipa::path(
    get,
    path = "/api/v1/telegram/users/{user_telegram_id}/messages",
    summary = "List messages authored by a user",
    description = "Returns indexed Telegram messages authored by one user, ordered by date descending then row id descending. Supports channel, text, and media filters with cursor pagination.",
    params(
        ("user_telegram_id" = i64, Path, description = "Telegram user id"),
        TelegramUserMessagesQuery
    ),
    tag = "telegram",
    operation_id = "listTelegramUserMessages",
    responses(
        (status = 200, description = "Messages authored by the user", body = TelegramMessagesListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "User not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_user_messages(
    State(app_state): State<AppState>,
    Path(user_telegram_id): Path<i64>,
    Query(query): Query<TelegramUserMessagesQuery>,
) -> Result<Json<TelegramMessagesListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let user = find_user_by_telegram_id(db.as_ref(), user_telegram_id).await?;
    let limit = resolve_limit(query.limit);

    let mut builder = TelegramMessages::find()
        .filter(telegram_messages::Column::FromUserId.eq(user.id))
        .order_by_desc(telegram_messages::Column::Date)
        .order_by_desc(telegram_messages::Column::Id);

    if let Some(channel_telegram_id) = query.channel_telegram_id {
        let Some(channel) =
            find_channel_by_telegram_id_optional(db.as_ref(), channel_telegram_id).await?
        else {
            return Ok(Json(TelegramMessagesListResponse {
                data: Vec::new(),
                next_cursor: None,
                has_more: false,
            }));
        };
        builder = builder.filter(telegram_messages::Column::ChannelId.eq(channel.id));
    }

    if let Some(message_type) = query
        .message_type
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        builder = builder.filter(telegram_messages::Column::MessageType.eq(message_type));
    }

    if let Some(has_media) = query.has_media {
        builder = builder.filter(telegram_messages::Column::HasMedia.eq(has_media));
    }

    if let Some(search) = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        builder = builder.filter(telegram_messages::Column::Text.contains(search));
    }

    if let Some(date_after) = query.date_after.as_deref() {
        let date_after = parse_rfc3339_to_naive(date_after, "date_after")?;
        builder = builder.filter(telegram_messages::Column::Date.gte(date_after));
    }
    if let Some(date_before) = query.date_before.as_deref() {
        let date_before = parse_rfc3339_to_naive(date_before, "date_before")?;
        builder = builder.filter(telegram_messages::Column::Date.lte(date_before));
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: DateIdCursor = decode_cursor(cursor)?;
        let date = cursor_epoch_to_naive(cursor.date_epoch, "user messages cursor")?;
        builder = builder.filter(
            Condition::any()
                .add(telegram_messages::Column::Date.lt(date))
                .add(
                    Condition::all()
                        .add(telegram_messages::Column::Date.eq(date))
                        .add(telegram_messages::Column::Id.lt(cursor.id)),
                ),
        );
    }

    let mut rows = builder
        .limit(limit + 1)
        .all(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?;

    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.pop();
    }

    let channel_ids: Vec<i64> = rows.iter().map(|row| row.channel_id).collect();
    let channels_by_id = channels_by_internal_id(db.as_ref(), &channel_ids).await?;

    let user_ids: Vec<i64> = rows.iter().filter_map(|row| row.from_user_id).collect();
    let users_by_id = users_by_internal_id(db.as_ref(), &user_ids).await?;

    let next_cursor = rows
        .last()
        .map(|row| {
            encode_cursor(&DateIdCursor {
                date_epoch: naive_to_epoch(row.date),
                id: row.id,
            })
        })
        .transpose()?;

    let data = rows
        .into_iter()
        .map(|row| {
            let channel = channels_by_id.get(&row.channel_id).ok_or_else(|| {
                ApiErrorResponse::internal_server_error("Message references unknown channel")
            })?;
            Ok(to_message_dto(row, channel.telegram_id, &users_by_id))
        })
        .collect::<Result<Vec<_>, ApiErrorResponse>>()?;

    Ok(Json(TelegramMessagesListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

fn to_channel_dto(row: telegram_channels::Model) -> TelegramChannelDto {
    TelegramChannelDto {
        channel_telegram_id: row.telegram_id,
        name: row.name,
        username: row.username,
        channel_type: row.r#type,
        created_at: to_rfc3339(row.created_at),
        updated_at: to_rfc3339(row.updated_at),
    }
}

fn to_user_dto(row: telegram_users::Model) -> TelegramUserDto {
    TelegramUserDto {
        user_telegram_id: row.telegram_id,
        username: row.username,
        display_name: row.display_name,
        created_at: to_rfc3339(row.created_at),
        updated_at: to_rfc3339(row.updated_at),
    }
}

fn to_message_dto(
    row: telegram_messages::Model,
    channel_telegram_id: i64,
    users_by_id: &HashMap<i64, telegram_users::Model>,
) -> TelegramMessageDto {
    let (user_telegram_id, from_username, from_display_name) = row
        .from_user_id
        .and_then(|id| users_by_id.get(&id))
        .map(|user| {
            (
                Some(user.telegram_id),
                user.username.clone(),
                user.display_name.clone(),
            )
        })
        .unwrap_or((None, None, None));

    TelegramMessageDto {
        channel_telegram_id,
        telegram_message_id: row.telegram_message_id,
        message_type: row.message_type,
        user_telegram_id,
        from_username,
        from_display_name,
        reply_to_telegram_message_id: row.reply_to_telegram_message_id,
        date: to_rfc3339(row.date),
        edited_date: row.edited_date.map(to_rfc3339),
        text: row.text,
        text_entities: row.text_entities,
        has_media: row.has_media,
        media_type: row.media_type,
        media_path: row.media_path,
        created_at: to_rfc3339(row.created_at),
        updated_at: to_rfc3339(row.updated_at),
    }
}

fn to_reaction_dto(
    row: telegram_reactions::Model,
    channel_telegram_id: i64,
    telegram_message_id: i64,
    users_by_id: &HashMap<i64, telegram_users::Model>,
) -> TelegramReactionDto {
    let user = users_by_id.get(&row.user_id);

    TelegramReactionDto {
        id: row.id,
        channel_telegram_id,
        telegram_message_id,
        user_telegram_id: user.map(|value| value.telegram_id),
        username: user.and_then(|value| value.username.clone()),
        display_name: user.and_then(|value| value.display_name.clone()),
        emoji: row.emoji,
        date: to_rfc3339(row.date),
        created_at: to_rfc3339(row.created_at),
    }
}

fn to_service_action_dto(
    row: telegram_service_actions::Model,
    channel_telegram_id: i64,
    telegram_message_id: i64,
    users_by_id: &HashMap<i64, telegram_users::Model>,
) -> TelegramServiceActionDto {
    let actor = row.actor_id.and_then(|id| users_by_id.get(&id));

    TelegramServiceActionDto {
        id: row.id,
        channel_telegram_id,
        telegram_message_id,
        action: row.action,
        actor_telegram_id: actor.map(|value| value.telegram_id),
        actor_username: actor.and_then(|value| value.username.clone()),
        actor_display_name: actor.and_then(|value| value.display_name.clone()),
        metadata: row.metadata,
        created_at: to_rfc3339(row.created_at),
    }
}

fn resolve_limit(limit: Option<u64>) -> u64 {
    limit.unwrap_or(20).clamp(1, 100)
}

#[allow(clippy::result_large_err)]
fn require_db(app_state: &AppState) -> Result<Arc<DatabaseConnection>, ApiErrorResponse> {
    app_state
        .db
        .clone()
        .ok_or_else(|| ApiErrorResponse::service_unavailable("Database connection unavailable"))
}

async fn find_channel_by_telegram_id_optional(
    db: &DatabaseConnection,
    channel_telegram_id: i64,
) -> Result<Option<telegram_channels::Model>, ApiErrorResponse> {
    TelegramChannels::find()
        .filter(telegram_channels::Column::TelegramId.eq(channel_telegram_id))
        .one(db)
        .await
        .map_err(ApiErrorResponse::from)
}

async fn find_channel_by_telegram_id(
    db: &DatabaseConnection,
    channel_telegram_id: i64,
) -> Result<telegram_channels::Model, ApiErrorResponse> {
    find_channel_by_telegram_id_optional(db, channel_telegram_id)
        .await?
        .ok_or_else(|| ApiErrorResponse::not_found("Telegram channel not found"))
}

async fn find_user_by_telegram_id_optional(
    db: &DatabaseConnection,
    user_telegram_id: i64,
) -> Result<Option<telegram_users::Model>, ApiErrorResponse> {
    TelegramUsers::find()
        .filter(telegram_users::Column::TelegramId.eq(user_telegram_id))
        .one(db)
        .await
        .map_err(ApiErrorResponse::from)
}

async fn find_user_by_telegram_id(
    db: &DatabaseConnection,
    user_telegram_id: i64,
) -> Result<telegram_users::Model, ApiErrorResponse> {
    find_user_by_telegram_id_optional(db, user_telegram_id)
        .await?
        .ok_or_else(|| ApiErrorResponse::not_found("Telegram user not found"))
}

async fn find_channel_message(
    db: &DatabaseConnection,
    channel_id: i64,
    telegram_message_id: i64,
) -> Result<telegram_messages::Model, ApiErrorResponse> {
    TelegramMessages::find()
        .filter(telegram_messages::Column::ChannelId.eq(channel_id))
        .filter(telegram_messages::Column::TelegramMessageId.eq(telegram_message_id))
        .one(db)
        .await
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::not_found("Telegram message not found"))
}

async fn users_by_internal_id(
    db: &DatabaseConnection,
    user_ids: &[i64],
) -> Result<HashMap<i64, telegram_users::Model>, ApiErrorResponse> {
    if user_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = TelegramUsers::find()
        .filter(telegram_users::Column::Id.is_in(user_ids.to_vec()))
        .all(db)
        .await
        .map_err(ApiErrorResponse::from)?;

    Ok(rows.into_iter().map(|row| (row.id, row)).collect())
}

async fn channels_by_internal_id(
    db: &DatabaseConnection,
    channel_ids: &[i64],
) -> Result<HashMap<i64, telegram_channels::Model>, ApiErrorResponse> {
    if channel_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = TelegramChannels::find()
        .filter(telegram_channels::Column::Id.is_in(channel_ids.to_vec()))
        .all(db)
        .await
        .map_err(ApiErrorResponse::from)?;

    Ok(rows.into_iter().map(|row| (row.id, row)).collect())
}

#[allow(clippy::result_large_err)]
fn parse_rfc3339_to_naive(
    value: &str,
    field_name: &str,
) -> Result<NaiveDateTime, ApiErrorResponse> {
    DateTime::parse_from_rfc3339(value)
        .map(|datetime| datetime.naive_utc())
        .map_err(|err| {
            ApiErrorResponse::bad_request(format!("Invalid {} value", field_name))
                .with_detail(err.to_string())
        })
}

#[allow(clippy::result_large_err)]
fn cursor_epoch_to_naive(
    epoch_seconds: i64,
    label: &str,
) -> Result<NaiveDateTime, ApiErrorResponse> {
    DateTime::from_timestamp(epoch_seconds, 0)
        .map(|datetime| datetime.naive_utc())
        .ok_or_else(|| ApiErrorResponse::bad_request(format!("Invalid {} timestamp", label)))
}

fn to_rfc3339(value: NaiveDateTime) -> String {
    value.and_utc().to_rfc3339()
}

fn naive_to_epoch(value: NaiveDateTime) -> i64 {
    value.and_utc().timestamp()
}

#[allow(clippy::result_large_err)]
fn encode_cursor<T: Serialize>(payload: &T) -> Result<String, ApiErrorResponse> {
    let bytes = serde_json::to_vec(payload).map_err(|err| {
        ApiErrorResponse::internal_server_error("Failed to encode pagination cursor")
            .with_detail(err.to_string())
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[allow(clippy::result_large_err)]
fn decode_cursor<T: DeserializeOwned>(cursor: &str) -> Result<T, ApiErrorResponse> {
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|err| {
        ApiErrorResponse::bad_request("Invalid cursor value").with_detail(err.to_string())
    })?;

    serde_json::from_slice::<T>(&bytes).map_err(|err| {
        ApiErrorResponse::bad_request("Invalid cursor payload").with_detail(err.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct CursorFixture {
        id: i64,
        key: String,
    }

    #[test]
    fn cursor_roundtrip_works() {
        let payload = CursorFixture {
            id: 7,
            key: "tg".to_string(),
        };
        let encoded = encode_cursor(&payload).expect("encode cursor");
        let decoded: CursorFixture = decode_cursor(&encoded).expect("decode cursor");

        assert_eq!(decoded, payload);
    }

    #[test]
    fn limit_is_bounded() {
        assert_eq!(resolve_limit(None), 20);
        assert_eq!(resolve_limit(Some(0)), 1);
        assert_eq!(resolve_limit(Some(15)), 15);
        assert_eq!(resolve_limit(Some(5_000)), 100);
    }
}
