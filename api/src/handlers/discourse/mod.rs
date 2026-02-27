//! Discourse read API handlers.

use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, NaiveDateTime, Utc};
use entities::{
    discourse::{
        discourse_categories, discourse_instances, discourse_post_likes, discourse_post_revisions,
        discourse_posts, discourse_topics, discourse_users,
    },
    prelude::*,
};
use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait,
    QueryFilter, QueryOrder, QuerySelect, Statement,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc};

#[derive(Debug, Serialize, Deserialize)]
struct StringCursor {
    key: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ExternalIdCursor {
    external_id: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct TopicsCursor {
    bumped_at_epoch: i64,
    external_id: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct TopicPostsCursor {
    post_number: i32,
    external_id: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct LikesCursor {
    created_at_epoch: i64,
    external_user_id: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct RevisionCursor {
    version: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct UserPostsCursor {
    created_at_epoch: i64,
    external_id: i32,
}

const CATEGORY_TOPICS_COUNTER_MISMATCH_KEY: &str = "category_topics_counter_mismatch_count";
const CATEGORY_POSTS_COUNTER_MISMATCH_KEY: &str = "category_posts_counter_mismatch_count";

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances",
    summary = "List indexed Discourse instances",
    description = "Returns registered Discourse instances known to the indexer. Results are ordered by instance key in ascending order and support cursor pagination.",
    params(CursorPaginationQuery),
    tag = "discourse",
    operation_id = "listDiscourseInstances",
    responses(
        (status = 200, description = "List registered discourse instances", body = InstancesListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_instances(
    State(app_state): State<AppState>,
    Query(query): Query<CursorPaginationQuery>,
) -> Result<Json<InstancesListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.limit);

    let mut builder = DiscourseInstances::find().order_by_asc(discourse_instances::Column::Key);
    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: StringCursor = decode_cursor(cursor)?;
        builder = builder.filter(discourse_instances::Column::Key.gt(cursor.key));
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
            encode_cursor(&StringCursor {
                key: row.key.clone(),
            })
        })
        .transpose()?;

    Ok(Json(InstancesListResponse {
        data: rows.into_iter().map(to_instance_dto).collect(),
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}",
    summary = "Get one Discourse instance",
    description = "Fetches a single registered Discourse instance by its stable `instance` key.",
    tag = "discourse",
    operation_id = "getDiscourseInstance",
    params(("instance" = String, Path, description = "Discourse instance key")),
    responses(
        (status = 200, description = "Discourse instance", body = DiscourseInstanceDto),
        (status = 404, description = "Instance not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_instance(
    State(app_state): State<AppState>,
    Path(instance): Path<String>,
) -> Result<Json<DiscourseInstanceDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let row = DiscourseInstances::find_by_id(instance.clone())
        .one(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::not_found("Discourse instance not found"))?;

    Ok(Json(to_instance_dto(row)))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/categories",
    summary = "List categories for an instance",
    description = "Returns indexed categories for a Discourse instance. Supports parent-category filtering, text search on name and slug, and cursor pagination.",
    params(
        ("instance" = String, Path, description = "Discourse instance key"),
        CategoriesQuery
    ),
    tag = "discourse",
    operation_id = "listDiscourseCategories",
    responses(
        (status = 200, description = "Categories", body = CategoriesListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Instance not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_categories(
    State(app_state): State<AppState>,
    Path(instance): Path<String>,
    Query(query): Query<CategoriesQuery>,
) -> Result<Json<CategoriesListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_instance_exists(db.as_ref(), &instance).await?;

    let limit = resolve_limit(query.limit);

    let mut builder = DiscourseCategories::find()
        .filter(discourse_categories::Column::Instance.eq(instance.clone()))
        .order_by_asc(discourse_categories::Column::ExternalId);

    if let Some(parent_id) = query.parent_category_id {
        builder = builder.filter(discourse_categories::Column::ParentCategoryId.eq(parent_id));
    }

    if let Some(search) = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        builder = builder.filter(
            Condition::any()
                .add(discourse_categories::Column::Name.contains(search))
                .add(discourse_categories::Column::Slug.contains(search)),
        );
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: ExternalIdCursor = decode_cursor(cursor)?;
        builder = builder.filter(discourse_categories::Column::ExternalId.gt(cursor.external_id));
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
            encode_cursor(&ExternalIdCursor {
                external_id: row.external_id,
            })
        })
        .transpose()?;

    Ok(Json(CategoriesListResponse {
        data: rows.into_iter().map(to_category_dto).collect(),
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/categories/{category_id}",
    summary = "Get one category by external id",
    description = "Fetches a single indexed category for the given instance and upstream Discourse `category_id`.",
    params(
        ("instance" = String, Path, description = "Discourse instance key"),
        ("category_id" = i32, Path, description = "External category id")
    ),
    tag = "discourse",
    operation_id = "getDiscourseCategory",
    responses(
        (status = 200, description = "Category", body = DiscourseCategoryDto),
        (status = 404, description = "Category not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_category(
    State(app_state): State<AppState>,
    Path((instance, category_id)): Path<(String, i32)>,
) -> Result<Json<DiscourseCategoryDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_instance_exists(db.as_ref(), &instance).await?;

    let row = DiscourseCategories::find()
        .filter(discourse_categories::Column::Instance.eq(instance))
        .filter(discourse_categories::Column::ExternalId.eq(category_id))
        .one(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::not_found("Category not found"))?;

    Ok(Json(to_category_dto(row)))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/topics",
    summary = "List topics for an instance",
    description = "Returns indexed topics sorted by `bumped_at` descending. Supports category and status filters, text search, bumped-at time bounds, and cursor pagination.",
    params(
        ("instance" = String, Path, description = "Discourse instance key"),
        TopicsQuery
    ),
    tag = "discourse",
    operation_id = "listDiscourseTopics",
    responses(
        (status = 200, description = "Topics", body = TopicsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Instance not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_topics(
    State(app_state): State<AppState>,
    Path(instance): Path<String>,
    Query(query): Query<TopicsQuery>,
) -> Result<Json<TopicsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_instance_exists(db.as_ref(), &instance).await?;

    let limit = resolve_limit(query.limit);

    let mut builder = DiscourseTopics::find()
        .filter(discourse_topics::Column::Instance.eq(instance.clone()))
        .order_by_desc(discourse_topics::Column::BumpedAt)
        .order_by_desc(discourse_topics::Column::ExternalId);

    if let Some(category_id) = query.category_id {
        builder = builder.filter(discourse_topics::Column::ExternalCategoryId.eq(category_id));
    }
    if let Some(pinned) = query.pinned {
        builder = builder.filter(discourse_topics::Column::Pinned.eq(pinned));
    }
    if let Some(closed) = query.closed {
        builder = builder.filter(discourse_topics::Column::Closed.eq(closed));
    }
    if let Some(archived) = query.archived {
        builder = builder.filter(discourse_topics::Column::Archived.eq(archived));
    }
    if let Some(visible) = query.visible {
        builder = builder.filter(discourse_topics::Column::Visible.eq(visible));
    }
    if let Some(search) = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        builder = builder.filter(
            Condition::any()
                .add(discourse_topics::Column::Title.contains(search))
                .add(discourse_topics::Column::FancyTitle.contains(search))
                .add(discourse_topics::Column::Slug.contains(search)),
        );
    }

    if let Some(bumped_after) = query.bumped_after.as_deref() {
        let bumped_after = parse_rfc3339_to_naive(bumped_after, "bumped_after")?;
        builder = builder.filter(discourse_topics::Column::BumpedAt.gte(bumped_after));
    }
    if let Some(bumped_before) = query.bumped_before.as_deref() {
        let bumped_before = parse_rfc3339_to_naive(bumped_before, "bumped_before")?;
        builder = builder.filter(discourse_topics::Column::BumpedAt.lte(bumped_before));
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: TopicsCursor = decode_cursor(cursor)?;
        let bumped_at = cursor_epoch_to_naive(cursor.bumped_at_epoch, "topics cursor")?;
        builder = builder.filter(
            Condition::any()
                .add(discourse_topics::Column::BumpedAt.lt(bumped_at))
                .add(
                    Condition::all()
                        .add(discourse_topics::Column::BumpedAt.eq(bumped_at))
                        .add(discourse_topics::Column::ExternalId.lt(cursor.external_id)),
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

    let next_cursor = rows
        .last()
        .map(|row| {
            encode_cursor(&TopicsCursor {
                bumped_at_epoch: naive_to_epoch(row.bumped_at),
                external_id: row.external_id,
            })
        })
        .transpose()?;

    Ok(Json(TopicsListResponse {
        data: rows.into_iter().map(to_topic_dto).collect(),
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/topics/{topic_id}",
    summary = "Get one topic by external id",
    description = "Fetches a single indexed topic for the given instance and upstream Discourse `topic_id`.",
    params(
        ("instance" = String, Path, description = "Discourse instance key"),
        ("topic_id" = i32, Path, description = "External topic id")
    ),
    tag = "discourse",
    operation_id = "getDiscourseTopic",
    responses(
        (status = 200, description = "Topic", body = DiscourseTopicDto),
        (status = 404, description = "Topic not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_topic(
    State(app_state): State<AppState>,
    Path((instance, topic_id)): Path<(String, i32)>,
) -> Result<Json<DiscourseTopicDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_instance_exists(db.as_ref(), &instance).await?;

    let row = DiscourseTopics::find()
        .filter(discourse_topics::Column::Instance.eq(instance))
        .filter(discourse_topics::Column::ExternalId.eq(topic_id))
        .one(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::not_found("Topic not found"))?;

    Ok(Json(to_topic_dto(row)))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/topics/{topic_id}/posts",
    summary = "List posts for a topic",
    description = "Returns posts for a topic ordered by `post_number` ascending. Supports username filtering, raw text search, and cursor pagination.",
    params(
        ("instance" = String, Path, description = "Discourse instance key"),
        ("topic_id" = i32, Path, description = "External topic id"),
        TopicPostsQuery
    ),
    tag = "discourse",
    operation_id = "listTopicPosts",
    responses(
        (status = 200, description = "Posts in topic", body = PostsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Topic not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_topic_posts(
    State(app_state): State<AppState>,
    Path((instance, topic_id)): Path<(String, i32)>,
    Query(query): Query<TopicPostsQuery>,
) -> Result<Json<PostsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_topic_exists(db.as_ref(), &instance, topic_id).await?;
    let limit = resolve_limit(query.limit);

    let mut builder = DiscoursePosts::find()
        .filter(discourse_posts::Column::Instance.eq(instance.clone()))
        .filter(discourse_posts::Column::ExternalTopicId.eq(topic_id))
        .order_by_asc(discourse_posts::Column::PostNumber)
        .order_by_asc(discourse_posts::Column::ExternalId);

    if let Some(username) = query
        .username
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        builder = builder.filter(discourse_posts::Column::Username.eq(username));
    }
    if let Some(search) = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        builder = builder.filter(discourse_posts::Column::Raw.contains(search));
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: TopicPostsCursor = decode_cursor(cursor)?;
        builder = builder.filter(
            Condition::any()
                .add(discourse_posts::Column::PostNumber.gt(cursor.post_number))
                .add(
                    Condition::all()
                        .add(discourse_posts::Column::PostNumber.eq(cursor.post_number))
                        .add(discourse_posts::Column::ExternalId.gt(cursor.external_id)),
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

    let next_cursor = rows
        .last()
        .map(|row| {
            encode_cursor(&TopicPostsCursor {
                post_number: row.post_number,
                external_id: row.external_id,
            })
        })
        .transpose()?;

    Ok(Json(PostsListResponse {
        data: rows.into_iter().map(to_post_dto).collect(),
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/posts/{post_id}",
    summary = "Get one post by external id",
    description = "Fetches a single indexed post for the given instance and upstream Discourse `post_id`.",
    params(
        ("instance" = String, Path, description = "Discourse instance key"),
        ("post_id" = i32, Path, description = "External post id")
    ),
    tag = "discourse",
    operation_id = "getDiscoursePost",
    responses(
        (status = 200, description = "Post", body = DiscoursePostDto),
        (status = 404, description = "Post not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_post(
    State(app_state): State<AppState>,
    Path((instance, post_id)): Path<(String, i32)>,
) -> Result<Json<DiscoursePostDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let row = DiscoursePosts::find()
        .filter(discourse_posts::Column::Instance.eq(instance))
        .filter(discourse_posts::Column::ExternalId.eq(post_id))
        .one(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::not_found("Post not found"))?;

    Ok(Json(to_post_dto(row)))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/posts/{post_id}/likes",
    summary = "List likes for a post",
    description = "Returns indexed like events for a post ordered by like timestamp descending, with best-effort username enrichment from indexed users.",
    params(
        ("instance" = String, Path, description = "Discourse instance key"),
        ("post_id" = i32, Path, description = "External post id"),
        CursorPaginationQuery
    ),
    tag = "discourse",
    operation_id = "listPostLikes",
    responses(
        (status = 200, description = "Post likes", body = PostLikesListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Post not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_post_likes(
    State(app_state): State<AppState>,
    Path((instance, post_id)): Path<(String, i32)>,
    Query(query): Query<CursorPaginationQuery>,
) -> Result<Json<PostLikesListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_post_exists(db.as_ref(), &instance, post_id).await?;
    let limit = resolve_limit(query.limit);

    let mut builder = DiscoursePostLikes::find()
        .filter(discourse_post_likes::Column::Instance.eq(instance.clone()))
        .filter(discourse_post_likes::Column::ExternalPostId.eq(post_id))
        .order_by_desc(discourse_post_likes::Column::CreatedAt)
        .order_by_desc(discourse_post_likes::Column::ExternalUserId);

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: LikesCursor = decode_cursor(cursor)?;
        let created_at = cursor_epoch_to_naive(cursor.created_at_epoch, "likes cursor")?;
        builder = builder.filter(
            Condition::any()
                .add(discourse_post_likes::Column::CreatedAt.lt(created_at))
                .add(
                    Condition::all()
                        .add(discourse_post_likes::Column::CreatedAt.eq(created_at))
                        .add(
                            discourse_post_likes::Column::ExternalUserId
                                .lt(cursor.external_user_id),
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

    let user_ids: Vec<i32> = rows.iter().map(|row| row.external_user_id).collect();
    let usernames = usernames_by_external_id(db.as_ref(), &instance, &user_ids).await?;

    let next_cursor = rows
        .last()
        .map(|row| {
            encode_cursor(&LikesCursor {
                created_at_epoch: naive_to_epoch(row.created_at),
                external_user_id: row.external_user_id,
            })
        })
        .transpose()?;

    Ok(Json(PostLikesListResponse {
        data: rows
            .into_iter()
            .map(|row| DiscoursePostLikeDto {
                instance: row.instance,
                external_post_id: row.external_post_id,
                external_user_id: row.external_user_id,
                username: usernames.get(&row.external_user_id).cloned(),
                created_at: to_rfc3339(row.created_at),
            })
            .collect(),
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/posts/{post_id}/revisions",
    summary = "List revisions for a post",
    description = "Returns indexed edit-history revisions for a post ordered by revision `version` ascending.",
    params(
        ("instance" = String, Path, description = "Discourse instance key"),
        ("post_id" = i32, Path, description = "External post id"),
        CursorPaginationQuery
    ),
    tag = "discourse",
    operation_id = "listPostRevisions",
    responses(
        (status = 200, description = "Post revisions", body = PostRevisionsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Post not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_post_revisions(
    State(app_state): State<AppState>,
    Path((instance, post_id)): Path<(String, i32)>,
    Query(query): Query<CursorPaginationQuery>,
) -> Result<Json<PostRevisionsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_post_exists(db.as_ref(), &instance, post_id).await?;
    let limit = resolve_limit(query.limit);

    let mut builder = DiscoursePostRevisions::find()
        .filter(discourse_post_revisions::Column::Instance.eq(instance.clone()))
        .filter(discourse_post_revisions::Column::ExternalPostId.eq(post_id))
        .order_by_asc(discourse_post_revisions::Column::Version);

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: RevisionCursor = decode_cursor(cursor)?;
        builder = builder.filter(discourse_post_revisions::Column::Version.gt(cursor.version));
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
            encode_cursor(&RevisionCursor {
                version: row.version,
            })
        })
        .transpose()?;

    Ok(Json(PostRevisionsListResponse {
        data: rows.into_iter().map(to_revision_dto).collect(),
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/users",
    summary = "List users for an instance",
    description = "Returns indexed users for a Discourse instance. Supports text search over username/name, optional trust-level filtering, and cursor pagination.",
    params(
        ("instance" = String, Path, description = "Discourse instance key"),
        UsersQuery
    ),
    tag = "discourse",
    operation_id = "listDiscourseUsers",
    responses(
        (status = 200, description = "Users", body = UsersListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Instance not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_users(
    State(app_state): State<AppState>,
    Path(instance): Path<String>,
    Query(query): Query<UsersQuery>,
) -> Result<Json<UsersListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_instance_exists(db.as_ref(), &instance).await?;
    let limit = resolve_limit(query.limit);

    let mut builder = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(instance.clone()))
        .order_by_asc(discourse_users::Column::ExternalId);

    if let Some(search) = query.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        builder = builder.filter(
            Condition::any()
                .add(discourse_users::Column::Username.contains(search))
                .add(discourse_users::Column::Name.contains(search)),
        );
    }
    if let Some(trust_level) = query.trust_level {
        builder = builder.filter(discourse_users::Column::TrustLevel.eq(trust_level));
    }

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: ExternalIdCursor = decode_cursor(cursor)?;
        builder = builder.filter(discourse_users::Column::ExternalId.gt(cursor.external_id));
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
            encode_cursor(&ExternalIdCursor {
                external_id: row.external_id,
            })
        })
        .transpose()?;

    Ok(Json(UsersListResponse {
        data: rows.into_iter().map(to_user_dto).collect(),
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/users/{user_id}",
    summary = "Get one user by external id",
    description = "Fetches a single indexed Discourse user by instance key and upstream `user_id`.",
    params(
        ("instance" = String, Path, description = "Discourse instance key"),
        ("user_id" = i32, Path, description = "External user id")
    ),
    tag = "discourse",
    operation_id = "getDiscourseUser",
    responses(
        (status = 200, description = "User", body = DiscourseUserDto),
        (status = 404, description = "User not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_user(
    State(app_state): State<AppState>,
    Path((instance, user_id)): Path<(String, i32)>,
) -> Result<Json<DiscourseUserDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let row = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(instance))
        .filter(discourse_users::Column::ExternalId.eq(user_id))
        .one(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::not_found("User not found"))?;

    Ok(Json(to_user_dto(row)))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/users/{user_id}/posts",
    summary = "List posts authored by a user",
    description = "Returns indexed posts authored by a user, ordered by creation time descending, with cursor pagination.",
    params(
        ("instance" = String, Path, description = "Discourse instance key"),
        ("user_id" = i32, Path, description = "External user id"),
        UserPostsQuery
    ),
    tag = "discourse",
    operation_id = "listUserPosts",
    responses(
        (status = 200, description = "User posts", body = PostsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "User not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_user_posts(
    State(app_state): State<AppState>,
    Path((instance, user_id)): Path<(String, i32)>,
    Query(query): Query<UserPostsQuery>,
) -> Result<Json<PostsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_user_exists(db.as_ref(), &instance, user_id).await?;
    let limit = resolve_limit(query.limit);

    let mut builder = DiscoursePosts::find()
        .filter(discourse_posts::Column::Instance.eq(instance.clone()))
        .filter(discourse_posts::Column::ExternalUserId.eq(user_id))
        .order_by_desc(discourse_posts::Column::CreatedAt)
        .order_by_desc(discourse_posts::Column::ExternalId);

    if let Some(cursor) = query.cursor.as_deref() {
        let cursor: UserPostsCursor = decode_cursor(cursor)?;
        let created_at = cursor_epoch_to_naive(cursor.created_at_epoch, "user posts cursor")?;
        builder = builder.filter(
            Condition::any()
                .add(discourse_posts::Column::CreatedAt.lt(created_at))
                .add(
                    Condition::all()
                        .add(discourse_posts::Column::CreatedAt.eq(created_at))
                        .add(discourse_posts::Column::ExternalId.lt(cursor.external_id)),
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

    let next_cursor = rows
        .last()
        .map(|row| {
            encode_cursor(&UserPostsCursor {
                created_at_epoch: naive_to_epoch(row.created_at),
                external_id: row.external_id,
            })
        })
        .transpose()?;

    Ok(Json(PostsListResponse {
        data: rows.into_iter().map(to_post_dto).collect(),
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/sync-lag",
    summary = "Get sync lag diagnostics",
    description = "Returns integrity and coverage checks comparing denormalized counters versus indexed rows for one Discourse instance.",
    params(("instance" = String, Path, description = "Discourse instance key")),
    tag = "discourse-ops",
    operation_id = "getDiscourseSyncLag",
    responses(
        (status = 200, description = "Sync lag diagnostics", body = SyncLagResponse),
        (status = 404, description = "Instance not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_sync_lag(
    State(app_state): State<AppState>,
    Path(instance): Path<String>,
) -> Result<Json<SyncLagResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_instance_exists(db.as_ref(), &instance).await?;

    let topic_posts_expected = scalar_i64(
        db.as_ref(),
        "SELECT COALESCE(SUM(posts_count), 0)::bigint AS value FROM discourse.discourse_topics WHERE instance = $1",
        &instance,
    )
    .await?;
    let topic_posts_actual = scalar_i64(
        db.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM discourse.discourse_posts WHERE instance = $1",
        &instance,
    )
    .await?;

    let topic_post_counter_mismatches = scalar_i64(
        db.as_ref(),
        "
        SELECT COUNT(*)::bigint AS value
        FROM discourse.discourse_topics t
        LEFT JOIN (
            SELECT external_topic_id, COUNT(*)::bigint AS actual_posts
            FROM discourse.discourse_posts
            WHERE instance = $1
            GROUP BY external_topic_id
        ) p ON p.external_topic_id = t.external_id
        WHERE t.instance = $1
          AND COALESCE(p.actual_posts, 0) <> t.posts_count::bigint
        ",
        &instance,
    )
    .await?;

    let category_topic_counter_mismatches = scalar_i64(
        db.as_ref(),
        "
        SELECT COUNT(*)::bigint AS value
        FROM discourse.discourse_categories c
        LEFT JOIN (
            SELECT external_category_id, COUNT(*)::bigint AS actual_topics
            FROM discourse.discourse_topics
            WHERE instance = $1
            GROUP BY external_category_id
        ) t ON t.external_category_id = c.external_id
        WHERE c.instance = $1
          AND COALESCE(t.actual_topics, 0) <> c.topic_count::bigint
        ",
        &instance,
    )
    .await?;

    let category_post_counter_mismatches = scalar_i64(
        db.as_ref(),
        "
        SELECT COUNT(*)::bigint AS value
        FROM discourse.discourse_categories c
        LEFT JOIN (
            SELECT t.external_category_id, COUNT(p.external_id)::bigint AS actual_posts
            FROM discourse.discourse_topics t
            LEFT JOIN discourse.discourse_posts p
              ON p.instance = t.instance
             AND p.external_topic_id = t.external_id
            WHERE t.instance = $1
            GROUP BY t.external_category_id
        ) cp ON cp.external_category_id = c.external_id
        WHERE c.instance = $1
          AND COALESCE(cp.actual_posts, 0) <> c.post_count::bigint
        ",
        &instance,
    )
    .await?;

    let likes_expected = scalar_i64(
        db.as_ref(),
        "SELECT COALESCE(SUM(like_count), 0)::bigint AS value FROM discourse.discourse_topics WHERE instance = $1",
        &instance,
    )
    .await?;
    let likes_actual = scalar_i64(
        db.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM discourse.discourse_post_likes WHERE instance = $1",
        &instance,
    )
    .await?;

    let revisions_expected = scalar_i64(
        db.as_ref(),
        "
        SELECT COALESCE(SUM(GREATEST(version - 1, 0)), 0)::bigint AS value
        FROM discourse.discourse_posts
        WHERE instance = $1 AND can_view_edit_history = true
        ",
        &instance,
    )
    .await?;
    let revisions_actual = scalar_i64(
        db.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM discourse.discourse_post_revisions WHERE instance = $1",
        &instance,
    )
    .await?;

    let orphan_post_user_refs = scalar_i64(
        db.as_ref(),
        "
        SELECT COUNT(*)::bigint AS value
        FROM discourse.discourse_posts p
        LEFT JOIN discourse.discourse_users u
          ON u.instance = p.instance
         AND u.external_id = p.external_user_id
        WHERE p.instance = $1
          AND u.external_id IS NULL
        ",
        &instance,
    )
    .await?;

    let checks = vec![
        build_check(
            "topic_posts_total",
            "Total topic posts_count sum vs actual posts rows",
            topic_posts_expected,
            topic_posts_actual,
            None,
        ),
        build_check(
            "topic_posts_counter_mismatch_count",
            "Topics where posts_count differs from actual topic post rows",
            0,
            topic_post_counter_mismatches,
            None,
        ),
        build_check(
            CATEGORY_TOPICS_COUNTER_MISMATCH_KEY,
            "Categories where topic_count differs from actual topic rows",
            0,
            category_topic_counter_mismatches,
            Some(
                "Category counters represent upstream totals and can stay mismatched while full backfill is incomplete."
                    .to_string(),
            ),
        ),
        build_check(
            CATEGORY_POSTS_COUNTER_MISMATCH_KEY,
            "Categories where post_count differs from posts grouped via topics",
            0,
            category_post_counter_mismatches,
            Some(
                "Category counters represent upstream totals and can stay mismatched while full backfill is incomplete."
                    .to_string(),
            ),
        ),
        build_check(
            "topic_likes_total",
            "Total topic like_count sum vs actual post like rows",
            likes_expected,
            likes_actual,
            Some("Topic like_count may drift while indexing is in progress.".to_string()),
        ),
        build_check(
            "post_revisions_total",
            "Expected revisions (from post version history) vs actual revision rows",
            revisions_expected,
            revisions_actual,
            Some(
                "Revision history availability varies per post and can lag transiently."
                    .to_string(),
            ),
        ),
        build_check(
            "orphan_post_user_refs",
            "Posts that reference a missing user row",
            0,
            orphan_post_user_refs,
            None,
        ),
    ];

    let summary = summarize_sync_lag_checks(&checks);

    Ok(Json(SyncLagResponse {
        instance,
        generated_at: Utc::now().to_rfc3339(),
        summary,
        checks,
    }))
}

fn build_check(
    key: &str,
    description: &str,
    expected: i64,
    actual: i64,
    caveat: Option<String>,
) -> SyncLagCheck {
    let delta = actual - expected;
    let ratio = if expected > 0 {
        Some(actual as f64 / expected as f64)
    } else {
        None
    };

    SyncLagCheck {
        key: key.to_string(),
        description: description.to_string(),
        expected,
        actual,
        delta,
        ratio,
        status: if delta == 0 {
            "ok".to_string()
        } else {
            "out_of_sync".to_string()
        },
        caveat,
    }
}

fn summarize_sync_lag_checks(checks: &[SyncLagCheck]) -> SyncLagSummary {
    let (integrity_out_of_sync_checks, coverage_out_of_sync_checks) =
        checks.iter().filter(|check| check.delta != 0).fold(
            (0_u32, 0_u32),
            |(integrity_count, coverage_count), check| {
                if is_coverage_check_key(&check.key) {
                    (integrity_count, coverage_count + 1)
                } else {
                    (integrity_count + 1, coverage_count)
                }
            },
        );

    let max_abs_delta = checks
        .iter()
        .map(|check| check.delta.unsigned_abs())
        .max()
        .unwrap_or_default() as i64;

    SyncLagSummary {
        total_checks: checks.len() as u32,
        out_of_sync_checks: integrity_out_of_sync_checks + coverage_out_of_sync_checks,
        integrity_out_of_sync_checks,
        coverage_out_of_sync_checks,
        max_abs_delta,
    }
}

fn is_coverage_check_key(key: &str) -> bool {
    matches!(
        key,
        CATEGORY_TOPICS_COUNTER_MISMATCH_KEY | CATEGORY_POSTS_COUNTER_MISMATCH_KEY
    )
}

fn to_instance_dto(row: discourse_instances::Model) -> DiscourseInstanceDto {
    DiscourseInstanceDto {
        key: row.key,
        base_url: row.base_url,
        enabled: row.enabled,
        created_at: row.created_at.map(to_rfc3339),
        updated_at: row.updated_at.map(to_rfc3339),
    }
}

fn to_category_dto(row: discourse_categories::Model) -> DiscourseCategoryDto {
    DiscourseCategoryDto {
        instance: row.instance,
        external_id: row.external_id,
        name: row.name,
        color: row.color,
        text_color: row.text_color,
        slug: row.slug,
        topic_count: row.topic_count,
        post_count: row.post_count,
        description: row.description,
        description_text: row.description_text,
        topics_day: row.topics_day,
        topics_week: row.topics_week,
        topics_month: row.topics_month,
        topics_year: row.topics_year,
        topics_all_time: row.topics_all_time,
        parent_category_id: row.parent_category_id,
        position: row.position,
        read_restricted: row.read_restricted,
    }
}

fn to_topic_dto(row: discourse_topics::Model) -> DiscourseTopicDto {
    DiscourseTopicDto {
        instance: row.instance,
        external_id: row.external_id,
        title: row.title,
        fancy_title: row.fancy_title,
        slug: row.slug,
        posts_count: row.posts_count,
        reply_count: row.reply_count,
        created_at: to_rfc3339(row.created_at),
        last_posted_at: to_rfc3339(row.last_posted_at),
        bumped_at: to_rfc3339(row.bumped_at),
        pinned: row.pinned,
        pinned_globally: row.pinned_globally,
        visible: row.visible,
        closed: row.closed,
        archived: row.archived,
        views: row.views,
        like_count: row.like_count,
        category_id: row.external_category_id,
        highest_post_number: row.highest_post_number,
        archetype: row.archetype,
        image_url: row.image_url,
    }
}

fn to_post_dto(row: discourse_posts::Model) -> DiscoursePostDto {
    DiscoursePostDto {
        instance: row.instance,
        external_id: row.external_id,
        name: row.name,
        username: row.username,
        created_at: to_rfc3339(row.created_at),
        post_number: row.post_number,
        post_type: row.post_type,
        updated_at: to_rfc3339(row.updated_at),
        reply_count: row.reply_count,
        reply_to_post_number: row.reply_to_post_number,
        quote_count: row.quote_count,
        incoming_link_count: row.incoming_link_count,
        reads: row.reads,
        readers_count: row.readers_count,
        score: row.score,
        topic_id: row.external_topic_id,
        topic_slug: row.topic_slug,
        display_username: row.display_username,
        primary_group_name: row.primary_group_name,
        flair_name: row.flair_name,
        flair_url: row.flair_url,
        flair_bg_color: row.flair_bg_color,
        flair_color: row.flair_color,
        version: row.version,
        user_id: row.external_user_id,
        can_view_edit_history: row.can_view_edit_history,
        deleted: row.deleted,
        raw: row.raw,
    }
}

fn to_user_dto(row: discourse_users::Model) -> DiscourseUserDto {
    DiscourseUserDto {
        instance: row.instance,
        external_id: row.external_id,
        username: row.username,
        name: row.name,
        avatar_template: row.avatar_template,
        title: row.title,
        likes_received: row.likes_received,
        likes_given: row.likes_given,
        topics_entered: row.topics_entered,
        topic_count: row.topic_count,
        post_count: row.post_count,
        posts_read: row.posts_read,
        days_visited: row.days_visited,
        trust_level: row.trust_level,
    }
}

fn to_revision_dto(row: discourse_post_revisions::Model) -> DiscoursePostRevisionDto {
    DiscoursePostRevisionDto {
        instance: row.instance,
        external_post_id: row.external_post_id,
        version: row.version,
        created_at: to_rfc3339(row.created_at),
        username: row.username,
        edit_reason: row.edit_reason,
        raw_body_before: row.raw_body_before,
        raw_body_after: row.raw_body_after,
        raw_title_before: row.raw_title_before,
        raw_title_after: row.raw_title_after,
        body_changes: row.body_changes,
        title_changes: row.title_changes,
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

async fn ensure_instance_exists(
    db: &DatabaseConnection,
    instance: &str,
) -> Result<(), ApiErrorResponse> {
    let exists = DiscourseInstances::find_by_id(instance.to_string())
        .one(db)
        .await
        .map_err(ApiErrorResponse::from)?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err(ApiErrorResponse::not_found("Discourse instance not found"))
    }
}

async fn ensure_topic_exists(
    db: &DatabaseConnection,
    instance: &str,
    topic_id: i32,
) -> Result<(), ApiErrorResponse> {
    let exists = DiscourseTopics::find()
        .filter(discourse_topics::Column::Instance.eq(instance))
        .filter(discourse_topics::Column::ExternalId.eq(topic_id))
        .one(db)
        .await
        .map_err(ApiErrorResponse::from)?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err(ApiErrorResponse::not_found("Topic not found"))
    }
}

async fn ensure_post_exists(
    db: &DatabaseConnection,
    instance: &str,
    post_id: i32,
) -> Result<(), ApiErrorResponse> {
    let exists = DiscoursePosts::find()
        .filter(discourse_posts::Column::Instance.eq(instance))
        .filter(discourse_posts::Column::ExternalId.eq(post_id))
        .one(db)
        .await
        .map_err(ApiErrorResponse::from)?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err(ApiErrorResponse::not_found("Post not found"))
    }
}

async fn ensure_user_exists(
    db: &DatabaseConnection,
    instance: &str,
    user_id: i32,
) -> Result<(), ApiErrorResponse> {
    let exists = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(instance))
        .filter(discourse_users::Column::ExternalId.eq(user_id))
        .one(db)
        .await
        .map_err(ApiErrorResponse::from)?
        .is_some();
    if exists {
        Ok(())
    } else {
        Err(ApiErrorResponse::not_found("User not found"))
    }
}

async fn usernames_by_external_id(
    db: &DatabaseConnection,
    instance: &str,
    user_ids: &[i32],
) -> Result<HashMap<i32, String>, ApiErrorResponse> {
    if user_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(instance))
        .filter(discourse_users::Column::ExternalId.is_in(user_ids.to_vec()))
        .all(db)
        .await
        .map_err(ApiErrorResponse::from)?;

    Ok(rows
        .into_iter()
        .map(|user| (user.external_id, user.username))
        .collect())
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

async fn scalar_i64(
    db: &DatabaseConnection,
    sql: &str,
    instance: &str,
) -> Result<i64, ApiErrorResponse> {
    let result = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![instance.into()],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let Some(result) = result else {
        return Ok(0);
    };

    result.try_get("", "value").map_err(|err| {
        ApiErrorResponse::internal_server_error("Failed to decode aggregate query")
            .with_detail(err.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct CursorFixture {
        id: i32,
        key: String,
    }

    #[test]
    fn cursor_roundtrips() {
        let payload = CursorFixture {
            id: 42,
            key: "abc".to_string(),
        };
        let encoded = encode_cursor(&payload).expect("encode cursor");
        let decoded: CursorFixture = decode_cursor(&encoded).expect("decode cursor");

        assert_eq!(decoded, payload);
    }

    #[test]
    fn limit_is_bounded() {
        assert_eq!(resolve_limit(None), 20);
        assert_eq!(resolve_limit(Some(0)), 1);
        assert_eq!(resolve_limit(Some(10)), 10);
        assert_eq!(resolve_limit(Some(10_000)), 100);
    }

    #[test]
    fn sync_lag_summary_splits_integrity_and_coverage_checks() {
        let checks = vec![
            build_check(CATEGORY_TOPICS_COUNTER_MISMATCH_KEY, "", 0, 12, None),
            build_check(CATEGORY_POSTS_COUNTER_MISMATCH_KEY, "", 0, 0, None),
            build_check("topic_likes_total", "", 100, 98, None),
            build_check("orphan_post_user_refs", "", 0, 0, None),
        ];

        let summary = summarize_sync_lag_checks(&checks);

        assert_eq!(summary.total_checks, 4);
        assert_eq!(summary.out_of_sync_checks, 2);
        assert_eq!(summary.integrity_out_of_sync_checks, 1);
        assert_eq!(summary.coverage_out_of_sync_checks, 1);
        assert_eq!(summary.max_abs_delta, 12);
    }

    #[test]
    fn sync_lag_summary_treats_unknown_keys_as_integrity_checks() {
        let checks = vec![build_check("custom_check", "", 0, 3, None)];

        let summary = summarize_sync_lag_checks(&checks);

        assert_eq!(summary.out_of_sync_checks, 1);
        assert_eq!(summary.integrity_out_of_sync_checks, 1);
        assert_eq!(summary.coverage_out_of_sync_checks, 0);
    }
}
