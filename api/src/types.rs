//! API request/response types and DTOs

use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};

/// Health check response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    /// Service status.
    pub status: String,
    /// Current timestamp.
    pub timestamp: String,
    /// Version information.
    pub version: String,
    /// Database connectivity status.
    pub database: DatabaseHealth,
}

/// Database health information.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DatabaseHealth {
    /// Whether database is accessible.
    pub connected: bool,
    /// Database response time in milliseconds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_time_ms: Option<u64>,
    /// Optional error message if not connected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CursorPageMeta {
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct InstancesListResponse {
    pub data: Vec<DiscourseInstanceDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CategoriesListResponse {
    pub data: Vec<DiscourseCategoryDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TopicsListResponse {
    pub data: Vec<DiscourseTopicDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PostsListResponse {
    pub data: Vec<DiscoursePostDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UsersListResponse {
    pub data: Vec<DiscourseUserDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PostLikesListResponse {
    pub data: Vec<DiscoursePostLikeDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PostRevisionsListResponse {
    pub data: Vec<DiscoursePostRevisionDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscourseInstanceDto {
    pub key: String,
    pub base_url: String,
    pub enabled: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscourseCategoryDto {
    pub instance: String,
    pub external_id: i32,
    pub name: String,
    pub color: String,
    pub text_color: String,
    pub slug: String,
    pub topic_count: i32,
    pub post_count: i32,
    pub description: Option<String>,
    pub description_text: Option<String>,
    pub topics_day: Option<i32>,
    pub topics_week: Option<i32>,
    pub topics_month: Option<i32>,
    pub topics_year: Option<i32>,
    pub topics_all_time: Option<i32>,
    pub parent_category_id: Option<i32>,
    pub position: Option<i32>,
    pub read_restricted: Option<bool>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscourseTopicDto {
    pub instance: String,
    pub external_id: i32,
    pub title: String,
    pub fancy_title: String,
    pub slug: String,
    pub posts_count: i32,
    pub reply_count: i32,
    pub created_at: String,
    pub last_posted_at: String,
    pub bumped_at: String,
    pub pinned: bool,
    pub pinned_globally: bool,
    pub visible: bool,
    pub closed: bool,
    pub archived: bool,
    pub views: i32,
    pub like_count: i32,
    pub category_id: i32,
    pub highest_post_number: i32,
    pub archetype: String,
    pub image_url: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscoursePostDto {
    pub instance: String,
    pub external_id: i32,
    pub name: Option<String>,
    pub username: String,
    pub created_at: String,
    pub post_number: i32,
    pub post_type: i32,
    pub updated_at: String,
    pub reply_count: i32,
    pub reply_to_post_number: Option<i32>,
    pub quote_count: i32,
    pub incoming_link_count: i32,
    pub reads: i32,
    pub readers_count: i32,
    pub score: f64,
    pub topic_id: i32,
    pub topic_slug: String,
    pub display_username: Option<String>,
    pub primary_group_name: Option<String>,
    pub flair_name: Option<String>,
    pub flair_url: Option<String>,
    pub flair_bg_color: Option<String>,
    pub flair_color: Option<String>,
    pub version: i32,
    pub user_id: i32,
    pub can_view_edit_history: bool,
    pub deleted: bool,
    pub raw: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscourseUserDto {
    pub instance: String,
    pub external_id: i32,
    pub username: String,
    pub name: Option<String>,
    pub avatar_template: String,
    pub title: Option<String>,
    pub likes_received: Option<i64>,
    pub likes_given: Option<i64>,
    pub topics_entered: Option<i64>,
    pub topic_count: Option<i64>,
    pub post_count: Option<i64>,
    pub posts_read: Option<i64>,
    pub days_visited: Option<i64>,
    pub trust_level: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscoursePostLikeDto {
    pub instance: String,
    pub external_post_id: i32,
    pub external_user_id: i32,
    pub username: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscoursePostRevisionDto {
    pub instance: String,
    pub external_post_id: i32,
    pub version: i32,
    pub created_at: String,
    pub username: String,
    pub edit_reason: Option<String>,
    pub raw_body_before: Option<String>,
    pub raw_body_after: Option<String>,
    pub raw_title_before: Option<String>,
    pub raw_title_after: Option<String>,
    pub body_changes: Option<serde_json::Value>,
    pub title_changes: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncLagSummary {
    pub total_checks: u32,
    pub out_of_sync_checks: u32,
    pub max_abs_delta: i64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncLagCheck {
    pub key: String,
    pub description: String,
    pub expected: i64,
    pub actual: i64,
    pub delta: i64,
    pub ratio: Option<f64>,
    pub status: String,
    pub caveat: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncLagResponse {
    pub instance: String,
    pub generated_at: String,
    pub summary: SyncLagSummary,
    pub checks: Vec<SyncLagCheck>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct CursorPaginationQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct CategoriesQuery {
    #[param(minimum = 1, maximum = 100)]
    pub limit: Option<u64>,
    pub cursor: Option<String>,
    pub parent_category_id: Option<i32>,
    pub q: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct TopicsQuery {
    #[param(minimum = 1, maximum = 100)]
    pub limit: Option<u64>,
    pub cursor: Option<String>,
    pub q: Option<String>,
    pub category_id: Option<i32>,
    pub pinned: Option<bool>,
    pub closed: Option<bool>,
    pub archived: Option<bool>,
    pub visible: Option<bool>,
    /// ISO-8601 datetime lower bound for bumped_at.
    pub bumped_after: Option<String>,
    /// ISO-8601 datetime upper bound for bumped_at.
    pub bumped_before: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct TopicPostsQuery {
    #[param(minimum = 1, maximum = 100)]
    pub limit: Option<u64>,
    pub cursor: Option<String>,
    pub username: Option<String>,
    pub q: Option<String>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct UsersQuery {
    #[param(minimum = 1, maximum = 100)]
    pub limit: Option<u64>,
    pub cursor: Option<String>,
    pub q: Option<String>,
    pub trust_level: Option<i32>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct UserPostsQuery {
    #[param(minimum = 1, maximum = 100)]
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}
