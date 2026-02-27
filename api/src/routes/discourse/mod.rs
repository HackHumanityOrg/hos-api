//! Discourse API routes.

use crate::{handlers::discourse, state::AppState};
use axum::{routing::get, Router};

pub fn discourse_routes() -> Router<AppState> {
    Router::new()
        .route("/discourse/instances", get(discourse::list_instances))
        .route(
            "/discourse/instances/{instance}",
            get(discourse::get_instance),
        )
        .route(
            "/discourse/instances/{instance}/categories",
            get(discourse::list_categories),
        )
        .route(
            "/discourse/instances/{instance}/categories/{category_id}",
            get(discourse::get_category),
        )
        .route(
            "/discourse/instances/{instance}/topics",
            get(discourse::list_topics),
        )
        .route(
            "/discourse/instances/{instance}/topics/{topic_id}",
            get(discourse::get_topic),
        )
        .route(
            "/discourse/instances/{instance}/topics/{topic_id}/posts",
            get(discourse::list_topic_posts),
        )
        .route(
            "/discourse/instances/{instance}/posts/{post_id}",
            get(discourse::get_post),
        )
        .route(
            "/discourse/instances/{instance}/posts/{post_id}/likes",
            get(discourse::list_post_likes),
        )
        .route(
            "/discourse/instances/{instance}/posts/{post_id}/revisions",
            get(discourse::list_post_revisions),
        )
        .route(
            "/discourse/instances/{instance}/users",
            get(discourse::list_users),
        )
        .route(
            "/discourse/instances/{instance}/users/{user_id}",
            get(discourse::get_user),
        )
        .route(
            "/discourse/instances/{instance}/users/{user_id}/posts",
            get(discourse::list_user_posts),
        )
        .route(
            "/discourse/instances/{instance}/sync-lag",
            get(discourse::get_sync_lag),
        )
}
