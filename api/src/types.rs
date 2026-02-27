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
    pub integrity_out_of_sync_checks: u32,
    pub coverage_out_of_sync_checks: u32,
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

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearProposalsListResponse {
    pub data: Vec<VenearProposalDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearProposalEventsListResponse {
    pub data: Vec<VenearProposalEventDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearVotesListResponse {
    pub data: Vec<VenearVoteDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearAccountBalancesListResponse {
    pub data: Vec<VenearAccountBalanceDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearLockupsListResponse {
    pub data: Vec<VenearLockupDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearLockupActionsListResponse {
    pub data: Vec<VenearLockupActionDto>,
    pub next_cursor: Option<String>,
    pub has_more: bool,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearProposalDto {
    pub id: i64,
    pub proposal_id: i64,
    pub latest_event_type: String,
    pub account_id: Option<String>,
    pub proposer_id: Option<String>,
    pub reviewer_id: Option<String>,
    pub voting_start_time_sec: Option<i64>,
    pub voting_start_time: Option<String>,
    pub snapshot_block_height: Option<i64>,
    pub snapshot_root: Option<String>,
    pub snapshot_total_venear: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub payload: Option<serde_json::Value>,
    pub block_height: i64,
    pub block_timestamp_ns: i64,
    pub block_timestamp: String,
    pub shard_id: i32,
    pub receipt_id: String,
    pub execution_outcome_id: String,
    pub predecessor_account_id: String,
    pub contract_id: String,
    pub log_index: i32,
    pub data_index: i32,
    pub created_at: String,
    pub created_at_epoch: i64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearProposalEventDto {
    pub id: i64,
    pub proposal_id: i64,
    pub event_type: String,
    pub account_id: Option<String>,
    pub proposer_id: Option<String>,
    pub reviewer_id: Option<String>,
    pub voting_start_time_sec: Option<i64>,
    pub voting_start_time: Option<String>,
    pub snapshot_block_height: Option<i64>,
    pub snapshot_root: Option<String>,
    pub snapshot_total_venear: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub payload: Option<serde_json::Value>,
    pub block_height: i64,
    pub block_timestamp_ns: i64,
    pub block_timestamp: String,
    pub shard_id: i32,
    pub receipt_id: String,
    pub execution_outcome_id: String,
    pub predecessor_account_id: String,
    pub contract_id: String,
    pub log_index: i32,
    pub data_index: i32,
    pub created_at: String,
    pub created_at_epoch: i64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearVoteDto {
    pub id: i64,
    pub proposal_id: i64,
    pub account_id: String,
    pub vote: i32,
    pub account_balance: String,
    pub event_type: String,
    pub payload: Option<serde_json::Value>,
    pub block_height: i64,
    pub block_timestamp_ns: i64,
    pub block_timestamp: String,
    pub shard_id: i32,
    pub receipt_id: String,
    pub execution_outcome_id: String,
    pub predecessor_account_id: String,
    pub contract_id: String,
    pub log_index: i32,
    pub data_index: i32,
    pub created_at: String,
    pub created_at_epoch: i64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearAccountBalanceDto {
    pub id: i64,
    pub account_id: String,
    pub event_type: String,
    pub delta_amount: Option<String>,
    pub total_balance: String,
    pub locked_near_balance: Option<String>,
    pub lockup_update_nonce: Option<i64>,
    pub lockup_version: Option<i64>,
    pub block_height: i64,
    pub block_timestamp_ns: i64,
    pub block_timestamp: String,
    pub shard_id: i32,
    pub receipt_id: String,
    pub execution_outcome_id: String,
    pub predecessor_account_id: String,
    pub contract_id: String,
    pub log_index: i32,
    pub data_index: i32,
    pub payload: Option<serde_json::Value>,
    pub created_at: String,
    pub created_at_epoch: i64,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearLockupDto {
    pub lockup_account_id: String,
    pub owner_account_id: String,
    pub lockup_version: Option<i64>,
    pub discovery_source: String,
    pub discovered_at_height: i64,
    pub discovered_at_timestamp_ns: i64,
    pub discovered_at_timestamp: String,
    pub last_seen_height: i64,
    pub last_seen_timestamp_ns: i64,
    pub last_seen_timestamp: String,
    pub is_active: bool,
    pub deleted_at_height: Option<i64>,
    pub deleted_at_timestamp_ns: Option<i64>,
    pub deleted_at_timestamp: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearLockupActionDto {
    pub id: i64,
    pub lockup_account_id: String,
    pub owner_account_id: Option<String>,
    pub predecessor_account_id: String,
    pub receiver_account_id: String,
    pub method_name: String,
    pub action_source: String,
    pub action_index: i32,
    pub args_json: Option<serde_json::Value>,
    pub args_base64: String,
    pub attached_deposit: String,
    pub transaction_hash: Option<String>,
    pub receipt_id: String,
    pub execution_outcome_id: String,
    pub success: bool,
    pub block_height: i64,
    pub block_timestamp_ns: i64,
    pub block_timestamp: String,
    pub shard_id: i32,
    pub staking_pool_account_id: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
    pub created_at_epoch: i64,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct VenearLockupsQuery {
    #[param(minimum = 1, maximum = 100)]
    pub limit: Option<u64>,
    pub cursor: Option<String>,
    pub owner_account_id: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Deserialize, IntoParams)]
pub struct VenearLockupActionsQuery {
    #[param(minimum = 1, maximum = 100)]
    pub limit: Option<u64>,
    pub cursor: Option<String>,
    pub method_name: Option<String>,
    pub success: Option<bool>,
}
