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

/// Cursor pagination metadata shared by list endpoints.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CursorPageMeta {
    /// Opaque cursor to request the next page; `null` when there is no next page.
    pub next_cursor: Option<String>,
    /// Indicates whether additional rows are available after this page.
    pub has_more: bool,
}

/// Paginated response of registered Discourse instances.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct InstancesListResponse {
    /// Page of instance rows.
    pub data: Vec<DiscourseInstanceDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of Discourse categories.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CategoriesListResponse {
    /// Page of category rows.
    pub data: Vec<DiscourseCategoryDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of Discourse topics.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TopicsListResponse {
    /// Page of topic rows.
    pub data: Vec<DiscourseTopicDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of Discourse posts.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PostsListResponse {
    /// Page of post rows.
    pub data: Vec<DiscoursePostDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of Discourse users.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UsersListResponse {
    /// Page of user rows.
    pub data: Vec<DiscourseUserDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of Discourse post-like events.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PostLikesListResponse {
    /// Page of like rows.
    pub data: Vec<DiscoursePostLikeDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of Discourse post revisions.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PostRevisionsListResponse {
    /// Page of revision rows.
    pub data: Vec<DiscoursePostRevisionDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Indexed Discourse instance metadata.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscourseInstanceDto {
    pub key: String,
    pub base_url: String,
    pub enabled: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

/// Indexed Discourse category row.
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

/// Indexed Discourse topic row.
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

/// Indexed Discourse post row.
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

/// Indexed Discourse user row.
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

/// Indexed Discourse post-like row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscoursePostLikeDto {
    pub instance: String,
    pub external_post_id: i32,
    pub external_user_id: i32,
    pub username: Option<String>,
    pub created_at: String,
}

/// Indexed Discourse post revision row.
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

/// High-level aggregate of sync-lag checks.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncLagSummary {
    /// Total number of checks produced in `checks`.
    pub total_checks: u32,
    /// Number of checks currently marked as `out_of_sync`.
    pub out_of_sync_checks: u32,
    /// Number of integrity checks marked as `out_of_sync`.
    pub integrity_out_of_sync_checks: u32,
    /// Number of coverage checks marked as `out_of_sync`.
    pub coverage_out_of_sync_checks: u32,
    /// Largest absolute delta across all checks.
    pub max_abs_delta: i64,
}

/// Detailed sync-lag check row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncLagCheck {
    /// Stable check identifier.
    pub key: String,
    /// Human-readable meaning of the check.
    pub description: String,
    /// Expected count/value used as baseline.
    pub expected: i64,
    /// Actual indexed count/value observed.
    pub actual: i64,
    /// Difference `actual - expected`.
    pub delta: i64,
    /// Optional ratio `actual / expected` when expected is positive.
    pub ratio: Option<f64>,
    /// Check status (`ok` or `out_of_sync`).
    pub status: String,
    /// Optional caveat for checks that may drift during partial backfill.
    pub caveat: Option<String>,
}

/// Sync-lag diagnostics payload for one Discourse instance.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct SyncLagResponse {
    /// Discourse instance key used for diagnostics.
    pub instance: String,
    /// RFC 3339 timestamp when diagnostics were generated.
    pub generated_at: String,
    /// Aggregated check summary.
    pub summary: SyncLagSummary,
    /// Per-check diagnostic rows.
    pub checks: Vec<SyncLagCheck>,
}

/// Common cursor pagination query used by multiple list endpoints.
#[derive(Debug, Deserialize, IntoParams)]
pub struct CursorPaginationQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
}

/// Query options for listing categories of one Discourse instance.
#[derive(Debug, Deserialize, IntoParams)]
pub struct CategoriesQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Filter by upstream Discourse parent category id.
    pub parent_category_id: Option<i32>,
    /// Case-insensitive substring match on category name or slug.
    pub q: Option<String>,
}

/// Query options for listing topics of one Discourse instance.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TopicsQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Case-insensitive substring match on topic title or slug.
    pub q: Option<String>,
    /// Filter by upstream Discourse category id.
    pub category_id: Option<i32>,
    /// Filter to pinned topics only (or unpinned when false).
    pub pinned: Option<bool>,
    /// Filter to closed topics only (or open when false).
    pub closed: Option<bool>,
    /// Filter to archived topics only (or non-archived when false).
    pub archived: Option<bool>,
    /// Filter to visible topics only (or hidden when false).
    pub visible: Option<bool>,
    /// ISO-8601 datetime lower bound for bumped_at.
    pub bumped_after: Option<String>,
    /// ISO-8601 datetime upper bound for bumped_at.
    pub bumped_before: Option<String>,
}

/// Query options for listing posts under a single topic.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TopicPostsQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Filter to posts authored by this username.
    pub username: Option<String>,
    /// Case-insensitive substring match on raw post body.
    pub q: Option<String>,
}

/// Query options for listing users of one Discourse instance.
#[derive(Debug, Deserialize, IntoParams)]
pub struct UsersQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Case-insensitive substring match on username or display name.
    pub q: Option<String>,
    /// Filter by Discourse trust level.
    pub trust_level: Option<i32>,
}

/// Query options for listing posts authored by one user.
#[derive(Debug, Deserialize, IntoParams)]
pub struct UserPostsQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
}

/// Paginated response of latest proposal state rows.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearProposalsListResponse {
    /// Page of proposal rows.
    pub data: Vec<VenearProposalDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of proposal event rows.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearProposalEventsListResponse {
    /// Page of proposal event rows.
    pub data: Vec<VenearProposalEventDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of proposal vote rows.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearVotesListResponse {
    /// Page of vote rows.
    pub data: Vec<VenearVoteDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of account balance mutation rows.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearAccountBalancesListResponse {
    /// Page of account balance rows.
    pub data: Vec<VenearAccountBalanceDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of discovered lockup rows.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearLockupsListResponse {
    /// Page of lockup rows.
    pub data: Vec<VenearLockupDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of lockup action rows.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearLockupActionsListResponse {
    /// Page of lockup action rows.
    pub data: Vec<VenearLockupActionDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of delegation action rows.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearDelegationEventsListResponse {
    /// Page of delegation rows.
    pub data: Vec<VenearDelegationEventDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Latest-state projection for a veNEAR proposal.
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

/// Event row from the proposal event stream.
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

/// Vote row recorded for a proposal.
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

/// Account balance mutation row for veNEAR governance balances.
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

/// Discovered veNEAR lockup account lifecycle row.
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

/// Indexed action row tied to a lockup account.
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

/// Indexed delegation action row tied to a governance account.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearDelegationEventDto {
    pub id: i64,
    pub delegator_account_id: String,
    pub delegatee_account_id: Option<String>,
    pub event_type: String,
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
    pub contract_id: String,
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
    pub created_at_epoch: i64,
}

/// Query options for listing discovered veNEAR lockup accounts.
#[derive(Debug, Deserialize, IntoParams)]
pub struct VenearLockupsQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Filter by owner account id.
    pub owner_account_id: Option<String>,
    /// Filter by active lockup lifecycle state.
    pub is_active: Option<bool>,
}

/// Query options for listing actions linked to one lockup account.
#[derive(Debug, Deserialize, IntoParams)]
pub struct VenearLockupActionsQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Filter by lockup action method name.
    pub method_name: Option<String>,
    /// Filter by execution success status.
    pub success: Option<bool>,
}

/// Query options for listing delegation actions linked to one account.
#[derive(Debug, Deserialize, IntoParams)]
pub struct VenearDelegationEventsQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Filter by delegation event type (`delegate_all`, `undelegate`).
    pub event_type: Option<String>,
    /// Filter by execution success status.
    pub success: Option<bool>,
}

/// Paginated response of indexed Telegram channels.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramChannelsListResponse {
    /// Page of channel rows.
    pub data: Vec<TelegramChannelDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of indexed Telegram users.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramUsersListResponse {
    /// Page of Telegram user rows.
    pub data: Vec<TelegramUserDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of indexed Telegram messages.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramMessagesListResponse {
    /// Page of Telegram message rows.
    pub data: Vec<TelegramMessageDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of indexed Telegram reactions.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramReactionsListResponse {
    /// Page of Telegram reaction rows.
    pub data: Vec<TelegramReactionDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of indexed Telegram service actions.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramServiceActionsListResponse {
    /// Page of Telegram service-action rows.
    pub data: Vec<TelegramServiceActionDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Indexed Telegram channel metadata.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramChannelDto {
    pub telegram_id: i64,
    pub name: String,
    pub username: Option<String>,
    pub channel_type: String,
    pub created_at: String,
    pub updated_at: String,
}

/// Indexed Telegram user metadata.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramUserDto {
    pub telegram_id: i64,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Indexed Telegram message row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramMessageDto {
    pub channel_telegram_id: i64,
    pub telegram_message_id: i64,
    pub message_type: String,
    pub from_user_telegram_id: Option<i64>,
    pub from_username: Option<String>,
    pub from_display_name: Option<String>,
    pub reply_to_telegram_message_id: Option<i64>,
    pub date: String,
    pub edited_date: Option<String>,
    pub text: Option<String>,
    pub text_entities: Option<serde_json::Value>,
    pub has_media: bool,
    pub media_type: Option<String>,
    pub media_path: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Indexed Telegram message reaction row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramReactionDto {
    pub id: i64,
    pub channel_telegram_id: i64,
    pub telegram_message_id: i64,
    pub user_telegram_id: Option<i64>,
    pub username: Option<String>,
    pub display_name: Option<String>,
    pub emoji: String,
    pub date: String,
    pub created_at: String,
}

/// Indexed Telegram service-action row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramServiceActionDto {
    pub id: i64,
    pub channel_telegram_id: i64,
    pub telegram_message_id: i64,
    pub action: String,
    pub actor_telegram_id: Option<i64>,
    pub actor_username: Option<String>,
    pub actor_display_name: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub created_at: String,
}

/// Query options for listing indexed Telegram channels.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TelegramChannelsQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Case-insensitive substring match on channel name or username.
    pub q: Option<String>,
    /// Filter by Telegram channel type (`channel`, `group`, `user`).
    pub channel_type: Option<String>,
    /// Case-insensitive substring match on channel username.
    pub username: Option<String>,
}

/// Query options for listing indexed Telegram users.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TelegramUsersQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Case-insensitive substring match on username or display name.
    pub q: Option<String>,
}

/// Query options for listing indexed Telegram messages in one channel.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TelegramMessagesQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Case-insensitive substring match on message text.
    pub q: Option<String>,
    /// Filter to messages authored by this Telegram user id.
    pub from_user_telegram_id: Option<i64>,
    /// Filter by Telegram message type (`message`, `media`, `service`, `unknown`).
    pub message_type: Option<String>,
    /// Filter by whether the message has media.
    pub has_media: Option<bool>,
    /// RFC 3339 lower bound for message date.
    pub date_after: Option<String>,
    /// RFC 3339 upper bound for message date.
    pub date_before: Option<String>,
}

/// Query options for listing indexed Telegram reactions for one message.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TelegramReactionsQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Filter by reaction emoji.
    pub emoji: Option<String>,
    /// Filter by Telegram user id of the reacting user.
    pub user_telegram_id: Option<i64>,
}

/// Query options for listing indexed Telegram service actions for one message.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TelegramServiceActionsQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Filter by service action type.
    pub action: Option<String>,
    /// Filter by Telegram actor user id.
    pub actor_telegram_id: Option<i64>,
}

/// Query options for listing indexed Telegram messages authored by one user.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TelegramUserMessagesQuery {
    /// Number of results to return. Defaults to 20. Max 100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque cursor returned from previous page.
    pub cursor: Option<String>,
    /// Optional channel filter by Telegram channel id.
    pub channel_telegram_id: Option<i64>,
    /// Case-insensitive substring match on message text.
    pub q: Option<String>,
    /// Filter by Telegram message type (`message`, `media`, `service`, `unknown`).
    pub message_type: Option<String>,
    /// Filter by whether the message has media.
    pub has_media: Option<bool>,
    /// RFC 3339 lower bound for message date.
    pub date_after: Option<String>,
    /// RFC 3339 upper bound for message date.
    pub date_before: Option<String>,
}

/// Paginated response of identity persons.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct IdentityPersonsListResponse {
    /// Page of identity person rows.
    pub data: Vec<IdentityPersonDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Identity person row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct IdentityPersonDto {
    /// Stable identity person id.
    pub id: i64,
    /// Best-effort human-readable identity name.
    pub name: Option<String>,
    /// Creation time in RFC 3339.
    pub created_at: String,
    /// Last update time in RFC 3339.
    pub updated_at: String,
}

/// Identity account linked to a person.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct IdentityAccountDto {
    /// Registered platform id (e.g. `near`, `discourse`, `telegram`).
    pub platform_id: String,
    /// Platform scope (e.g. chain or instance key).
    pub scope: String,
    /// Stable external account identifier inside platform scope.
    pub external_id: String,
    /// Current known aliases for this account.
    pub current_aliases: Vec<String>,
}

/// Response containing all accounts linked to one person.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct IdentityPersonAccountsResponse {
    /// Identity person id used for lookup.
    pub person_id: i64,
    /// Accounts linked to this person (empty when no links exist yet).
    pub accounts: Vec<IdentityAccountDto>,
}
