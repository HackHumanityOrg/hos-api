//! Telegram API routes.

use crate::{handlers::telegram, state::AppState};
use axum::{routing::get, Router};

pub fn telegram_routes() -> Router<AppState> {
    Router::new()
        .route("/telegram/channels", get(telegram::list_channels))
        .route(
            "/telegram/channels/{channel_telegram_id}",
            get(telegram::get_channel),
        )
        .route(
            "/telegram/channels/{channel_telegram_id}/messages",
            get(telegram::list_channel_messages),
        )
        .route(
            "/telegram/channels/{channel_telegram_id}/messages/{telegram_message_id}",
            get(telegram::get_channel_message),
        )
        .route(
            "/telegram/channels/{channel_telegram_id}/messages/{telegram_message_id}/reactions",
            get(telegram::list_message_reactions),
        )
        .route(
            "/telegram/channels/{channel_telegram_id}/messages/{telegram_message_id}/service-actions",
            get(telegram::list_message_service_actions),
        )
        .route("/telegram/users", get(telegram::list_users))
        .route(
            "/telegram/users/{user_telegram_id}",
            get(telegram::get_user),
        )
        .route(
            "/telegram/users/{user_telegram_id}/messages",
            get(telegram::list_user_messages),
        )
}
