//! API request/response types and DTOs

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use utoipa::{IntoParams, ToSchema};

/// Health check response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    /// High-level service health status (`healthy`, `degraded`, or similar).
    pub status: String,
    /// Timestamp of the generated response in RFC 3339 format.
    pub timestamp: String,
    /// Semantic version of the running API crate.
    pub version: String,
    /// Result of the optional database connectivity probe.
    pub database: DatabaseHealth,
}

/// Database health information.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DatabaseHealth {
    /// Whether the API can currently reach the database.
    pub connected: bool,
    /// Database round-trip time in milliseconds, when probe completes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_time_ms: Option<u64>,
    /// Diagnostic message when database probe fails.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Cursor pagination metadata shared by list endpoints.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CursorPageMeta {
    /// Opaque cursor token for the next page.
    ///
    /// Clients should pass this value verbatim into the next request's `cursor`.
    pub next_cursor: Option<String>,
    /// Indicates whether one or more additional pages remain.
    ///
    /// If `true`, clients should continue using `next_cursor`.
    pub has_more: bool,
}

/// Paginated response of registered Discourse instances.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct InstancesListResponse {
    /// Returned page of Discourse instance rows.
    pub data: Vec<DiscourseInstanceDto>,
    /// Opaque cursor token for the next result page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists after this payload.
    pub has_more: bool,
}

/// Paginated response of Discourse categories.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct CategoriesListResponse {
    /// Returned page of Discourse category rows.
    pub data: Vec<DiscourseCategoryDto>,
    /// Opaque cursor token for the next result page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists after this payload.
    pub has_more: bool,
}

/// Paginated response of Discourse topics.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TopicsListResponse {
    /// Returned page of Discourse topic rows.
    pub data: Vec<DiscourseTopicDto>,
    /// Opaque cursor token for the next result page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists after this payload.
    pub has_more: bool,
}

/// Paginated response of Discourse posts.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PostsListResponse {
    /// Returned page of Discourse post rows.
    pub data: Vec<DiscoursePostDto>,
    /// Opaque cursor token for the next result page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists after this payload.
    pub has_more: bool,
}

/// Paginated response of Discourse users.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct UsersListResponse {
    /// Returned page of Discourse user rows.
    pub data: Vec<DiscourseUserDto>,
    /// Opaque cursor token for the next result page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists after this payload.
    pub has_more: bool,
}

/// Paginated response of Discourse post-like events.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PostLikesListResponse {
    /// Returned page of Discourse post-like rows.
    pub data: Vec<DiscoursePostLikeDto>,
    /// Opaque cursor token for the next result page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists after this payload.
    pub has_more: bool,
}

/// Paginated response of Discourse post revisions.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct PostRevisionsListResponse {
    /// Returned page of Discourse post-revision rows.
    pub data: Vec<DiscoursePostRevisionDto>,
    /// Opaque cursor token for the next result page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists after this payload.
    pub has_more: bool,
}

/// Indexed Discourse instance metadata.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscourseInstanceDto {
    /// Stable upstream instance identifier (for example, subdomain).
    pub instance: String,
    /// Base URL of the indexed Discourse instance.
    pub base_url: String,
    /// Whether the instance is enabled in the indexer pipeline.
    pub enabled: bool,
    /// Timestamp when the instance was first recorded, if available.
    pub created_at: Option<String>,
    /// Timestamp when the instance was last updated in index metadata.
    pub updated_at: Option<String>,
}

/// Indexed Discourse category row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscourseCategoryDto {
    /// Instance key owning this indexed category.
    pub instance: String,
    /// Upstream category id from Discourse.
    pub category_id: i32,
    /// Human-readable category name.
    pub name: String,
    /// UI accent color.
    pub color: String,
    /// Text accent color.
    pub text_color: String,
    /// URL slug used by Discourse for the category.
    pub slug: String,
    /// Number of topics in this category.
    pub topic_count: i32,
    /// Number of posts in this category.
    pub post_count: i32,
    /// HTML-formatted category description, when indexed.
    pub description: Option<String>,
    /// Plain text category description, when available.
    pub description_text: Option<String>,
    /// Topic count in the last day (or null if unknown).
    pub topics_day: Option<i32>,
    /// Topic count in the last week (or null if unknown).
    pub topics_week: Option<i32>,
    /// Topic count in the last month (or null if unknown).
    pub topics_month: Option<i32>,
    /// Topic count in the last year (or null if unknown).
    pub topics_year: Option<i32>,
    /// All-time topic count.
    pub topics_all_time: Option<i32>,
    /// Parent category id when this category is nested.
    pub parent_category_id: Option<i32>,
    /// Display position order in parent list.
    pub position: Option<i32>,
    /// Whether category access is restricted.
    pub read_restricted: Option<bool>,
}

/// Indexed Discourse topic row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscourseTopicDto {
    /// Instance key owning this topic.
    pub instance: String,
    /// Upstream topic id.
    pub topic_id: i32,
    /// Public title of the topic.
    pub title: String,
    /// Fancy title variant from Discourse.
    pub fancy_title: String,
    /// URL slug for the topic.
    pub slug: String,
    /// Total post count in the topic.
    pub posts_count: i32,
    /// Total number of replies.
    pub reply_count: i32,
    /// Topic creation timestamp in RFC 3339.
    pub created_at: String,
    /// Timestamp of last post activity in RFC 3339.
    pub last_posted_at: String,
    /// Timestamp for ranking field `bumped_at` in RFC 3339.
    pub bumped_at: String,
    /// Whether the topic is pinned.
    pub pinned: bool,
    /// Whether the topic is globally pinned.
    pub pinned_globally: bool,
    /// Whether the topic is visible.
    pub visible: bool,
    /// Whether the topic is closed to new replies.
    pub closed: bool,
    /// Whether the topic is archived.
    pub archived: bool,
    /// Number of views.
    pub views: i32,
    /// Number of likes recorded.
    pub like_count: i32,
    /// Upstream category id.
    pub category_id: i32,
    /// Highest post number in this topic.
    pub highest_post_number: i32,
    /// Discourse topic archetype.
    pub archetype: String,
    /// Optional image URL attached to topic.
    pub image_url: Option<String>,
}

/// Indexed Discourse post row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscoursePostDto {
    /// Instance key owning this post.
    pub instance: String,
    /// Upstream post id.
    pub post_id: i32,
    /// Author display name or null for some system posts.
    pub name: Option<String>,
    /// Author username.
    pub username: String,
    /// Post creation timestamp in RFC 3339.
    pub created_at: String,
    /// Sequence number of the post in the topic.
    pub post_number: i32,
    /// Upstream post type code.
    pub post_type: i32,
    /// Post last-updated timestamp in RFC 3339.
    pub updated_at: String,
    /// Reply count under this post.
    pub reply_count: i32,
    /// Parent post number when this is a reply.
    pub reply_to_post_number: Option<i32>,
    /// Number of quoted snippets.
    pub quote_count: i32,
    /// Incoming links to this post.
    pub incoming_link_count: i32,
    /// Number of raw reads.
    pub reads: i32,
    /// Number of readers who viewed this post.
    pub readers_count: i32,
    /// Relevance/quality score.
    pub score: f64,
    /// Upstream topic id containing this post.
    pub topic_id: i32,
    /// Upstream topic slug.
    pub topic_slug: String,
    /// Alternate display username for the post author.
    pub display_username: Option<String>,
    /// Primary group name when the post is posted on behalf of a group.
    pub primary_group_name: Option<String>,
    /// Flair text, when set.
    pub flair_name: Option<String>,
    /// Flair badge URL.
    pub flair_url: Option<String>,
    /// Flair background color.
    pub flair_bg_color: Option<String>,
    /// Flair text color.
    pub flair_color: Option<String>,
    /// Content parser/format revision of the post.
    pub version: i32,
    /// Upstream author user id.
    pub user_id: i32,
    /// Whether edit history is available for this post.
    pub can_view_edit_history: bool,
    /// Whether the post is marked deleted.
    pub deleted: bool,
    /// Optional markdown/raw source text.
    pub raw: Option<String>,
}

/// Indexed Discourse user row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscourseUserDto {
    /// Instance key owning this user index.
    pub instance: String,
    /// Upstream user id.
    pub user_id: i32,
    /// Username.
    pub username: String,
    /// Optional display name.
    pub name: Option<String>,
    /// Avatar URL template with width/height placeholders.
    pub avatar_template: String,
    /// Optional title field.
    pub title: Option<String>,
    /// Number of likes this user received.
    pub likes_received: Option<i64>,
    /// Number of likes this user gave.
    pub likes_given: Option<i64>,
    /// Number of topics entered.
    pub topics_entered: Option<i64>,
    /// Total topics created by user.
    pub topic_count: Option<i64>,
    /// Total posts created by user.
    pub post_count: Option<i64>,
    /// Posts read count.
    pub posts_read: Option<i64>,
    /// Days visited count.
    pub days_visited: Option<i64>,
    /// Trust level in Discourse.
    pub trust_level: Option<i32>,
}

/// Indexed Discourse post-like row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscoursePostLikeDto {
    /// Instance key owning this like event.
    pub instance: String,
    /// Upstream post id that was liked.
    pub post_id: i32,
    /// Upstream user id of liker.
    pub user_id: i32,
    /// Optional liker username (resolved via users table).
    pub username: Option<String>,
    /// Like creation timestamp in RFC 3339.
    pub created_at: String,
}

/// Indexed Discourse post revision row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DiscoursePostRevisionDto {
    /// Instance key owning this post revision.
    pub instance: String,
    /// Upstream post id for which the revision was created.
    pub post_id: i32,
    /// Sequential revision number.
    pub version: i32,
    /// Revision creation timestamp in RFC 3339.
    pub created_at: String,
    /// User name (or actor) responsible for revision.
    pub username: String,
    /// Optional reason supplied by actor for edit.
    pub edit_reason: Option<String>,
    /// Body content before the revision (optional and may be large).
    pub raw_body_before: Option<String>,
    /// Body content after the revision.
    pub raw_body_after: Option<String>,
    /// Title before the revision.
    pub raw_title_before: Option<String>,
    /// Title after the revision.
    pub raw_title_after: Option<String>,
    /// Optional raw body diff payload.
    pub body_changes: Option<serde_json::Value>,
    /// Optional raw title diff payload.
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
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to the closed interval 1..=100.
    /// - Larger values are reduced to 100, smaller values to 1.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    ///
    /// Provide this value exactly as returned by the preceding page.
    /// Omit this field for the first page.
    pub cursor: Option<String>,
}

/// Query options for listing latest veNEAR proposal states.
#[derive(Debug, Deserialize, IntoParams)]
pub struct VenearProposalsQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Filter latest proposal rows by latest event type.
    ///
    /// Examples: `create_proposal`, `proposal_approve_finalized`, `proposal_reject`.
    pub latest_event_type: Option<String>,
    /// Filter by proposal creator account id.
    ///
    /// Matches either the latest event `proposer_id` or create event proposer id.
    pub proposer_id: Option<String>,
}

/// Query options for listing veNEAR config history events.
#[derive(Debug, Deserialize, IntoParams)]
pub struct VenearConfigHistoryQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Optional exact contract filter (for example `venear.dao` or `vote.dao`).
    pub contract_id: Option<String>,
    /// Optional config field filter (`guardians` or `reviewer_ids`).
    pub field: Option<String>,
}

/// Query options for listing events of a single proposal.
#[derive(Debug, Deserialize, IntoParams)]
pub struct VenearProposalEventsQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Filter by proposal event type.
    pub event_type: Option<String>,
    /// Filter by actor account id.
    ///
    /// Matches `account_id` when present, otherwise proposer or reviewer id.
    pub actor_account_id: Option<String>,
}

/// Query options for listing effective votes of a single proposal.
#[derive(Debug, Deserialize, IntoParams)]
pub struct VenearProposalVotesQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Filter by voter account id.
    pub voter_account_id: Option<String>,
    /// Filter by selected voting option index.
    pub vote_option: Option<i32>,
}

/// Query options for listing veNEAR delegates.
#[derive(Debug, Deserialize, IntoParams)]
pub struct VenearDelegatesQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Prefix filter for delegate account id.
    pub account_id_prefix: Option<String>,
    /// Filter delegates by inbound delegation state.
    pub has_inbound_delegation: Option<bool>,
    /// Filter delegates by outbound delegation state.
    pub has_outbound_delegation: Option<bool>,
}

/// Query options for listing voting power history of one delegate.
#[derive(Debug, Deserialize, IntoParams)]
pub struct VenearDelegateVotingPowerQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Filter by voting power event type.
    pub event_type: Option<String>,
    /// Filter by proposal id for proposal-scoped events.
    pub proposal_id: Option<i64>,
}

/// Query options for listing delegation events of one delegate.
#[derive(Debug, Deserialize, IntoParams)]
pub struct VenearDelegateDelegationsQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Filter by delegation event type (`delegate_all` or `undelegate`).
    pub event_type: Option<String>,
    /// Filter relative direction (`inbound` or `outbound`) for the requested account.
    pub direction: Option<String>,
    /// Filter by counterparty account id.
    ///
    /// For outbound events this matches delegatee, for inbound events this matches delegator.
    pub counterparty_account_id: Option<String>,
    /// Filter by event success flag.
    pub success: Option<bool>,
}

/// Query options for listing categories of one Discourse instance.
#[derive(Debug, Deserialize, IntoParams)]
pub struct CategoriesQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Return only categories whose parent category id matches this value.
    /// Leave unset to include both root and nested categories.
    pub parent_category_id: Option<i32>,
    /// Substring match on category `name` and `slug`.
    ///
    /// Empty strings are ignored and treated as if no filter was supplied.
    pub q: Option<String>,
}

/// Query options for listing topics of one Discourse instance.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TopicsQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Substring match on topic `title`, `fancy_title`, and `slug`.
    ///
    /// Empty strings are ignored and treated as if no filter was supplied.
    pub q: Option<String>,
    /// Filter by upstream Discourse category id.
    pub category_id: Option<i32>,
    /// Filter topics by `pinned` state:
    /// - `true` for pinned only
    /// - `false` for unpinned only
    pub pinned: Option<bool>,
    /// Filter topics by `closed` state:
    /// - `true` for closed only
    /// - `false` for open only
    pub closed: Option<bool>,
    /// Filter topics by `archived` state:
    /// - `true` for archived only
    /// - `false` for active only
    pub archived: Option<bool>,
    /// Filter topics by `visible` state:
    /// - `true` for visible only
    /// - `false` for hidden only
    pub visible: Option<bool>,
    /// Lower bound for `bumped_at` (inclusive), RFC 3339 date-time string.
    ///
    /// Example: `2025-11-30T00:00:00Z`.
    pub bumped_after: Option<String>,
    /// Upper bound for `bumped_at` (inclusive), RFC 3339 date-time string.
    ///
    /// Example: `2025-11-30T23:59:59Z`.
    pub bumped_before: Option<String>,
}

/// Query options for listing posts under a single topic.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TopicPostsQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Filter to posts with an exact username match.
    pub username: Option<String>,
    /// Substring match on raw post body.
    pub q: Option<String>,
}

/// Query options for listing users of one Discourse instance.
#[derive(Debug, Deserialize, IntoParams)]
pub struct UsersQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Substring match on `username` and `name`.
    pub q: Option<String>,
    /// Exact match for Discourse trust level (integer scale from the upstream service).
    pub trust_level: Option<i32>,
}

/// Query options for listing posts authored by one user.
#[derive(Debug, Deserialize, IntoParams)]
pub struct UserPostsQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
}

/// Paginated response of veNEAR config history rows.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearConfigHistoryListResponse {
    /// Page of veNEAR config history rows.
    pub data: Vec<VenearConfigEntryDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Latest veNEAR config snapshot rows.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearConfigCurrentResponse {
    /// Latest config row per `(contract_id, field)`.
    pub data: Vec<VenearConfigEntryDto>,
}

/// veNEAR config event row normalized from governance method calls.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearConfigEntryDto {
    /// Internal primary key for deterministic ordering.
    pub id: i64,
    /// Contract account id where the config change was called.
    pub contract_id: String,
    /// Config field (`guardians` or `reviewer_ids`).
    pub field: String,
    /// New value assigned to the config field.
    pub value: Vec<String>,
    /// Normalized source (`init_new` or `set_method`).
    pub source: String,
    /// Raw contract method used (`new`, `set_guardians`, or `set_reviewer_ids`).
    pub method_name: String,
    /// Predecessor account id that submitted the call.
    pub predecessor_account_id: String,
    /// Optional transaction hash when captured at transaction level.
    pub transaction_hash: Option<String>,
    /// Receipt id associated with this action.
    pub receipt_id: String,
    /// Execution outcome id associated with this action.
    pub execution_outcome_id: String,
    /// Block height where the action was indexed.
    pub block_height: i64,
    /// Block timestamp in RFC 3339 format.
    pub block_timestamp: String,
}

/// Paginated response of latest proposal state rows.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearProposalsListResponse {
    /// Page of proposal rows.
    pub data: Vec<VenearProposalSummaryDto>,
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
    /// Page of proposal vote rows.
    pub data: Vec<VenearVoteDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of delegate rows.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearDelegatesListResponse {
    /// Page of delegate rows.
    pub data: Vec<VenearDelegateDto>,
    /// Opaque cursor for the next page.
    pub next_cursor: Option<String>,
    /// Indicates whether another page exists.
    pub has_more: bool,
}

/// Paginated response of delegate voting power history rows.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearDelegateVotingPowerEventsListResponse {
    /// Page of delegate voting power event rows.
    pub data: Vec<VenearDelegateVotingPowerEventDto>,
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

/// Latest-state projection for a veNEAR proposal in list views.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearProposalSummaryDto {
    /// Upstream proposal identifier.
    pub proposal_id: i64,
    /// Proposal title, when present.
    pub title: Option<String>,
    /// Proposal description, when present.
    pub description: Option<String>,
    /// Proposal link, when present.
    pub link: Option<String>,
    /// Voting options available for the proposal.
    pub voting_options: Vec<String>,
    /// Normalized proposal status (`created`, `approved`, `voting`, `finished`, `rejected`).
    pub status: String,
    /// Proposal creator account id.
    pub proposer_id: Option<String>,
    /// Reviewer account id, when available.
    pub reviewer_id: Option<String>,
    /// Proposal creation timestamp.
    pub created_at: Option<String>,
    /// Voting start timestamp.
    pub voting_start_at: Option<String>,
    /// Voting end timestamp.
    pub voting_end_at: Option<String>,
    /// Snapshot block height, when available.
    pub snapshot_block_height: Option<i64>,
    /// Snapshot Merkle root, when available.
    pub snapshot_root: Option<String>,
    /// Snapshot total veNEAR, stringified for precision.
    pub snapshot_total_venear: Option<String>,
    /// Number of currently effective votes.
    pub effective_votes_count: i64,
    /// Total voting power of currently effective votes.
    pub effective_votes_voting_power: String,
    /// Latest event type in the proposal stream.
    pub latest_event_type: String,
    /// Block height of the latest event.
    pub latest_block_height: i64,
    /// Block timestamp of the latest event.
    pub latest_block_timestamp: String,
}

/// Detailed proposal view.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearProposalDto {
    /// Upstream proposal identifier.
    pub proposal_id: i64,
    /// Proposal title, when present.
    pub title: Option<String>,
    /// Proposal description, when present.
    pub description: Option<String>,
    /// Proposal link, when present.
    pub link: Option<String>,
    /// Voting options available for the proposal.
    pub voting_options: Vec<String>,
    /// Normalized proposal status (`created`, `approved`, `voting`, `finished`, `rejected`).
    pub status: String,
    /// Proposal creator account id.
    pub proposer_id: Option<String>,
    /// Reviewer account id, when available.
    pub reviewer_id: Option<String>,
    /// Proposal creation timestamp.
    pub created_at: Option<String>,
    /// Voting start timestamp.
    pub voting_start_at: Option<String>,
    /// Voting end timestamp.
    pub voting_end_at: Option<String>,
    /// Snapshot block height, when available.
    pub snapshot_block_height: Option<i64>,
    /// Snapshot Merkle root, when available.
    pub snapshot_root: Option<String>,
    /// Snapshot total veNEAR, stringified for precision.
    pub snapshot_total_venear: Option<String>,
    /// Number of currently effective votes.
    pub effective_votes_count: i64,
    /// Total voting power of currently effective votes.
    pub effective_votes_voting_power: String,
    /// Aggregates per vote option.
    pub vote_aggregates: Vec<VenearVoteAggregateDto>,
    /// Metadata about the latest proposal event.
    pub latest_event: VenearProposalLatestEventDto,
}

/// Aggregated vote information for one voting option.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearVoteAggregateDto {
    /// Index of voting option.
    pub option_index: i32,
    /// Label of voting option, when available.
    pub option_label: Option<String>,
    /// Number of votes for this option.
    pub votes_count: i64,
    /// Total voting power for this option.
    pub voting_power: String,
}

/// Latest event metadata for a proposal.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearProposalLatestEventDto {
    /// Event type.
    pub event_type: String,
    /// Block height where the event was emitted.
    pub block_height: i64,
    /// Block timestamp in RFC 3339 format.
    pub block_timestamp: String,
    /// Optional transaction hash.
    pub transaction_hash: Option<String>,
    /// Receipt id associated with the event.
    pub receipt_id: String,
    /// Execution outcome id associated with the event.
    pub execution_outcome_id: String,
}

/// Event row from the proposal event stream.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearProposalEventDto {
    /// Internal primary key for ordering.
    pub id: i64,
    /// Proposal id associated with this event.
    pub proposal_id: i64,
    /// Event type emitted by the contract.
    pub event_type: String,
    /// Primary actor account id, when available.
    pub actor_account_id: Option<String>,
    /// Proposer account id, when available.
    pub proposer_id: Option<String>,
    /// Reviewer account id, when available.
    pub reviewer_id: Option<String>,
    /// Normalized proposal status inferred at this event.
    pub status: Option<String>,
    /// Proposal title, when available.
    pub title: Option<String>,
    /// Proposal description, when available.
    pub description: Option<String>,
    /// Proposal link, when available.
    pub link: Option<String>,
    /// Voting start timestamp, when available.
    pub voting_start_at: Option<String>,
    /// Voting duration in nanoseconds, when available.
    pub voting_duration_ns: Option<String>,
    /// Snapshot block height, when available.
    pub snapshot_block_height: Option<i64>,
    /// Snapshot root, when available.
    pub snapshot_root: Option<String>,
    /// Snapshot total veNEAR, when available.
    pub snapshot_total_venear: Option<String>,
    /// Block height where the event was emitted.
    pub block_height: i64,
    /// Block timestamp in RFC 3339 format.
    pub block_timestamp: String,
    /// Optional transaction hash.
    pub transaction_hash: Option<String>,
    /// Receipt id associated with the event.
    pub receipt_id: String,
    /// Execution outcome id associated with the event.
    pub execution_outcome_id: String,
}

/// Effective vote row recorded for a proposal.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearVoteDto {
    /// Internal primary key for ordering.
    pub id: i64,
    /// Proposal id receiving the vote.
    pub proposal_id: i64,
    /// Account id of the voter.
    pub voter_account_id: String,
    /// Numeric option index selected by the voter.
    pub vote_option: i32,
    /// Voting option label, when available.
    pub vote_label: Option<String>,
    /// Voting power of the voter at vote time.
    pub voting_power: String,
    /// Event type for the latest vote row (`add_vote` expected for effective votes).
    pub event_type: String,
    /// Whether this vote is currently effective.
    pub is_effective: bool,
    /// Block height where the vote event was emitted.
    pub block_height: i64,
    /// Block timestamp in RFC 3339 format.
    pub block_timestamp: String,
    /// Optional transaction hash.
    pub transaction_hash: Option<String>,
    /// Receipt id associated with the vote event.
    pub receipt_id: String,
    /// Execution outcome id associated with the vote event.
    pub execution_outcome_id: String,
}

/// Delegate row with own and delegated voting power.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearDelegateDto {
    /// Delegate account id.
    pub account_id: String,
    /// Voting power owned directly by this account.
    pub own_voting_power: String,
    /// Voting power delegated to this account from others.
    pub delegated_voting_power: String,
    /// Effective total voting power available to this account.
    pub total_voting_power: String,
    /// Number of accounts currently delegating to this account.
    pub delegators_count: i64,
    /// Delegate target if this account currently delegates outward.
    pub delegated_to_account_id: Option<String>,
    /// Whether this account currently has inbound delegation.
    pub has_inbound_delegation: bool,
    /// Whether this account currently delegates to another account.
    pub has_outbound_delegation: bool,
    /// Most recent timestamp where the account cast an effective vote.
    pub last_vote_at: Option<String>,
    /// Last observed timestamp from balance/delegation/vote activity.
    pub updated_at: String,
}

/// Voting power mutation row for a delegate account.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearDelegateVotingPowerEventDto {
    /// Internal primary key for ordering.
    pub id: i64,
    /// Delegate account id used for lookup.
    pub account_id: String,
    /// Event type for this mutation.
    pub event_type: String,
    /// Normalized reason label: `balance_increase`, `balance_decrease`,
    /// `delegation_received`, `delegation_given`, `delegation_returned`,
    /// `delegation_removed`, `vote_cast`, `vote_removed`, or `other`.
    pub reason: String,
    /// Proposal id when mutation is proposal-scoped.
    pub proposal_id: Option<i64>,
    /// Voting power after this mutation.
    pub voting_power: String,
    /// Voting power delta for this mutation, when available.
    pub delta_voting_power: Option<String>,
    /// Block height where this mutation was recorded.
    pub block_height: i64,
    /// Block timestamp in RFC 3339 format.
    pub block_timestamp: String,
    /// Optional transaction hash.
    pub transaction_hash: Option<String>,
    /// Receipt id associated with this mutation.
    pub receipt_id: String,
    /// Execution outcome id associated with this mutation.
    pub execution_outcome_id: String,
}

/// Delegation event row for a delegate account.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct VenearDelegationEventDto {
    /// Internal primary key for ordering.
    pub id: i64,
    /// Account id used for lookup.
    pub account_id: String,
    /// Direction relative to the requested account (`inbound` or `outbound`).
    pub direction: String,
    /// Delegation event type (`delegate_all` or `undelegate`).
    pub event_type: String,
    /// Delegator account id.
    pub delegator_account_id: String,
    /// Raw delegatee account id as stored in the event.
    pub delegatee_account_id: Option<String>,
    /// Delegatee account id with undelegate inference applied when possible.
    pub resolved_delegatee_account_id: Option<String>,
    /// Whether `resolved_delegatee_account_id` was inferred.
    pub delegatee_inferred: bool,
    /// Whether the action succeeded.
    pub success: bool,
    /// Block height where the event was emitted.
    pub block_height: i64,
    /// Block timestamp in RFC 3339 format.
    pub block_timestamp: String,
    /// Optional transaction hash.
    pub transaction_hash: Option<String>,
    /// Receipt id associated with the event.
    pub receipt_id: String,
    /// Execution outcome id associated with the event.
    pub execution_outcome_id: String,
}

/// Legacy Agora query options for proposal list endpoints.
#[derive(Debug, Deserialize, IntoParams)]
pub struct AgoraPaginationQuery {
    /// Requested page size. Invalid values fall back to 10.
    pub page_size: Option<String>,
    /// Requested page number. Invalid values fall back to 1.
    pub page: Option<String>,
}

/// Legacy Agora query options for pending proposals.
#[derive(Debug, Deserialize, IntoParams)]
pub struct AgoraPendingProposalsQuery {
    /// Optional proposal creator filter.
    pub created_by: Option<String>,
    /// Requested page size. Invalid values fall back to 10.
    pub page_size: Option<String>,
    /// Requested page number. Invalid values fall back to 1.
    pub page: Option<String>,
}

/// Legacy Agora query options for delegates listing.
#[derive(Debug, Deserialize, IntoParams)]
pub struct AgoraDelegatesQuery {
    /// Requested page size. Invalid values fall back to 10.
    pub page_size: Option<String>,
    /// Requested page number. Invalid values fall back to 1.
    pub page: Option<String>,
    /// Legacy ordering selector.
    pub order_by: Option<String>,
    /// Legacy filter selector.
    pub filter_by: Option<String>,
    /// Legacy issue-type filter, comma-separated.
    pub issue_type: Option<String>,
    /// Optional deterministic seed for weighted random ordering.
    pub sorting_seed: Option<String>,
}

/// Legacy Agora query options for delegate statement changes.
#[derive(Debug, Deserialize, IntoParams)]
pub struct AgoraDelegateStatementChangesQuery {
    /// Requested page number. Must be a positive integer when provided.
    pub page: Option<String>,
    /// Requested page size. Must be a positive integer when provided.
    pub offset: Option<String>,
    /// Optional lookback window in days.
    pub lookback_days: Option<String>,
}

/// Legacy Agora query options for delegate HOS activity.
#[derive(Debug, Deserialize, IntoParams)]
pub struct AgoraDelegateHosActivityQuery {
    /// Requested page size. Invalid values fall back to 10.
    pub page_size: Option<String>,
    /// Requested page number. Invalid values fall back to 1.
    pub page: Option<String>,
    /// Legacy network id parameter retained for compatibility.
    pub network_id: Option<String>,
    /// Legacy contract id parameter retained for compatibility.
    pub contract_id: Option<String>,
}

/// Legacy Agora error payload using `{ "error": ... }`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraLegacyErrorBody {
    /// Human-readable error string.
    pub error: String,
}

/// Legacy Agora error payload using `{ "message": ... }`.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraLegacyMessageBody {
    /// Human-readable error string.
    pub message: String,
}

/// Legacy Agora approved/pending proposals response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraProposalListResponse {
    /// Returned proposal rows.
    pub proposals: Vec<AgoraProposalDto>,
    /// Total number of matching rows before pagination.
    pub count: i64,
}

/// Legacy Agora proposal row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgoraProposalDto {
    /// Legacy proposal identifier, sourced from `receipt_id`.
    pub id: String,
    /// Proposal type derived from encoded metadata.
    pub proposal_type: String,
    /// Approval threshold in basis points.
    pub approval_threshold: String,
    /// Approval timestamp, when present.
    pub approved_at: Option<String>,
    /// Reviewer account id, when present.
    pub approver_id: Option<String>,
    /// Proposal creation timestamp, when present.
    pub created_at: Option<String>,
    /// Creator account id, when present.
    pub creator_id: Option<String>,
    /// Whether the proposal currently has any votes.
    pub has_vote: bool,
    /// Whether the proposal is approved.
    pub is_approved: bool,
    /// Whether the proposal is rejected.
    pub is_rejected: bool,
    /// Raw proposal description stored on-chain.
    pub proposal_description: Option<String>,
    /// Upstream proposal identifier as a string.
    pub proposal_id: Option<String>,
    /// Proposal title, when present.
    pub proposal_title: Option<String>,
    /// Proposal URL, when present.
    pub proposal_url: Option<String>,
    /// Receipt id associated with the create proposal action.
    pub receipt_id: String,
    /// Rejection timestamp, when present.
    pub rejected_at: Option<String>,
    /// Rejecter account id, when present.
    pub rejecter_id: Option<String>,
    /// Voting power currently counted as `for`.
    pub for_voting_power: Option<String>,
    /// Voting power currently counted as `against`.
    pub against_voting_power: Option<String>,
    /// Voting power currently counted as `abstain`.
    pub abstain_voting_power: Option<String>,
    /// Voting duration in nanoseconds, when present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voting_duration_ns: Option<String>,
    /// Snapshot total voting power at approval time.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_voting_power: Option<String>,
    /// Calculated quorum amount as a string.
    pub quorum_amount: String,
    /// Legacy derived status (`Created`, `Voting`, `Finished`, `Rejected`, `Unknown`).
    pub status: String,
    /// Voting start timestamp converted to nanoseconds.
    pub voting_start_time_ns: Option<String>,
    /// Proposal creation timestamp converted to nanoseconds.
    pub voting_created_at_ns: Option<String>,
}

/// Legacy Agora proposal quorum response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgoraProposalQuorumResponse {
    /// Calculated quorum amount as a string.
    pub quorum_amount: String,
}

/// Legacy Agora proposal votes response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraProposalVotesResponse {
    /// Returned vote rows.
    pub votes: Vec<AgoraProposalVoteDto>,
    /// Total number of matching rows before pagination.
    pub count: i64,
}

/// Legacy Agora proposal vote row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgoraProposalVoteDto {
    /// Voter account id.
    pub account_id: String,
    /// Voting power as a string.
    pub voting_power: String,
    /// Selected vote option as a string.
    pub vote_option: String,
}

/// Legacy Agora proposal charts response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraProposalChartsResponse {
    /// Returned chart points.
    pub data: Vec<AgoraProposalChartPointDto>,
}

/// Legacy Agora proposal chart point.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgoraProposalChartPointDto {
    /// Voter account id.
    pub account_id: String,
    /// Voting power as a string.
    pub voting_power: String,
    /// Selected vote option as a string.
    pub vote_option: String,
    /// Vote timestamp in ISO-8601 format.
    pub voted_at: Option<String>,
}

/// Legacy Agora proposal non-voters response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgoraProposalNonVotersResponse {
    /// Returned non-voter rows.
    pub non_voters: Vec<AgoraProposalNonVoterDto>,
    /// Total number of matching rows before pagination.
    pub count: i64,
}

/// Legacy Agora proposal non-voter row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgoraProposalNonVoterDto {
    /// Stable row identifier.
    pub id: String,
    /// Proposal identifier.
    pub proposal_id: String,
    /// Registered voter account id.
    pub registered_voter_id: String,
    /// Current voting power as a string.
    pub voting_power: String,
}

/// Legacy Agora vote changes response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraVoteChangesResponse {
    /// Vote change groups keyed by account id.
    pub vote_changes: Vec<AgoraVoteChangeDto>,
}

/// Legacy Agora vote change group.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraVoteChangeDto {
    /// Voter account id.
    pub account_id: String,
    /// Full vote history for the voter on the proposal.
    pub changes: Vec<AgoraVoteChangeEntryDto>,
}

/// Legacy Agora vote change entry.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraVoteChangeEntryDto {
    /// Selected vote option.
    pub vote_option: i32,
    /// Voting power as a string.
    pub voting_power: String,
    /// Vote timestamp in ISO-8601 format.
    pub voted_at: String,
    /// Block height, coerced to a plain number.
    pub block_height: i64,
}

/// Legacy Agora delegates list response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraDelegatesResponse {
    /// Returned delegate rows.
    pub delegates: Vec<AgoraDelegateSummaryDto>,
    /// Total number of matching rows before pagination.
    pub count: i64,
}

/// Legacy Agora top-issue entry.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraTopIssueDto {
    /// Legacy issue type label.
    pub r#type: String,
    /// Legacy issue value.
    pub value: String,
}

/// Legacy Agora delegate summary row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgoraDelegateSummaryDto {
    /// Delegate address.
    pub address: String,
    /// Current voting power as a string.
    pub voting_power: Option<String>,
    /// Proposal participation rate as a string.
    pub participation_rate: Option<String>,
    /// Twitter handle, unavailable in this compatibility layer.
    pub twitter: Option<String>,
    /// Discord handle, unavailable in this compatibility layer.
    pub discord: Option<String>,
    /// Warpcast handle, unavailable in this compatibility layer.
    pub warpcast: Option<String>,
    /// Delegate statement, unavailable in this compatibility layer.
    pub statement: Option<String>,
    /// Delegate top issues, unavailable in this compatibility layer.
    pub top_issues: Option<Vec<AgoraTopIssueDto>>,
    /// Whether the delegate is endorsed, unavailable in this compatibility layer.
    pub endorsed: Option<bool>,
    /// Legacy notification preferences blob, unavailable in this compatibility layer.
    #[schema(value_type = Object)]
    pub notification_preferences: Option<JsonValue>,
}

/// Legacy Agora delegate detail response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraDelegateDetailResponse {
    /// Detailed delegate row.
    pub delegate: AgoraDelegateDetailDto,
}

/// Legacy Agora delegate detail row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgoraDelegateDetailDto {
    /// Delegate address.
    pub address: String,
    /// Twitter handle, unavailable in this compatibility layer.
    pub twitter: Option<String>,
    /// Discord handle, unavailable in this compatibility layer.
    pub discord: Option<String>,
    /// Warpcast handle, unavailable in this compatibility layer.
    pub warpcast: Option<String>,
    /// Delegate statement, unavailable in this compatibility layer.
    pub statement: Option<String>,
    /// Delegate top issues, unavailable in this compatibility layer.
    pub top_issues: Option<Vec<AgoraTopIssueDto>>,
    /// Whether the delegate is endorsed, unavailable in this compatibility layer.
    pub endorsed: Option<bool>,
    /// Legacy notification preferences blob, unavailable in this compatibility layer.
    #[schema(value_type = Object)]
    pub notification_preferences: Option<JsonValue>,
    /// Current voting power as a string.
    pub voting_power: Option<String>,
    /// Number of effective `for` votes cast by the delegate.
    pub for_count: i64,
    /// Number of effective `against` votes cast by the delegate.
    pub against_count: i64,
    /// Number of effective `abstain` votes cast by the delegate.
    pub abstain_count: i64,
    /// Number of accounts currently delegating to this delegate.
    pub delegated_from_count: i64,
    /// Proposal participation rate as a string.
    pub participation_rate: Option<String>,
}

/// Legacy Agora delegate voting history response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraDelegateVotingHistoryResponse {
    /// Returned vote rows.
    pub votes: Vec<AgoraDelegateVotingHistoryDto>,
    /// Total number of matching rows before pagination.
    pub count: i64,
}

/// Legacy Agora delegate voting history row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgoraDelegateVotingHistoryDto {
    /// Selected vote option as a string.
    pub vote_option: Option<String>,
    /// Voting power as a string.
    pub voting_power: String,
    /// Delegate address.
    pub address: Option<String>,
    /// Vote timestamp in ISO-8601 format.
    pub voted_at: Option<String>,
    /// Proposal identifier as a string.
    pub proposal_id: Option<String>,
    /// Proposal title, when present.
    pub proposal_name: Option<String>,
}

/// Legacy Agora delegation events response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraDelegationEventsResponse {
    /// Returned delegation rows.
    pub events: Vec<AgoraDelegationEventDto>,
    /// Total number of matching rows before pagination.
    pub count: i64,
}

/// Legacy Agora delegation event row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgoraDelegationEventDto {
    /// Stable row identifier.
    pub id: String,
    /// Receipt id associated with the event.
    pub receipt_id: String,
    /// Event date coerced to an ISO-8601 timestamp string.
    pub event_date: Option<String>,
    /// Event timestamp in ISO-8601 format.
    pub event_timestamp: Option<String>,
    /// HOS contract account id.
    pub hos_contract_address: Option<String>,
    /// Delegator account id.
    pub delegator_id: String,
    /// Delegatee account id, when present.
    pub delegatee_id: Option<String>,
    /// Delegation method (`delegate_all` or `undelegate`).
    pub delegate_method: Option<String>,
    /// Token transfer event (`ft_mint` or `ft_burn`).
    pub delegate_event: Option<String>,
    /// Whether this is the latest event for the delegator.
    pub is_latest_delegator_event: bool,
    /// Event owner id, when present.
    pub owner_id: Option<String>,
    /// Delegated amount as a string.
    pub near_amount: Option<String>,
    /// Block height as a string.
    pub block_height: Option<String>,
    /// Block hash, when present.
    pub block_hash: Option<String>,
}

/// Legacy Agora delegate HOS activity response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgoraHosActivityResponse {
    /// Returned activity rows.
    pub hos_activity: Vec<AgoraHosActivityDto>,
    /// Total number of matching rows before pagination.
    pub count: i64,
}

/// Legacy Agora delegate HOS activity row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AgoraHosActivityDto {
    /// Receipt id associated with the event.
    pub receipt_id: String,
    /// Block height as a string.
    pub block_height: Option<String>,
    /// Event date coerced to an ISO-8601 timestamp string.
    pub event_date: Option<String>,
    /// Raw near amount as indexed.
    pub near_amount: Option<String>,
    /// Raw locked near balance as indexed.
    pub locked_near_balance: Option<String>,
    /// Legacy normalized transaction type label.
    pub transaction_type: String,
}

/// Legacy Agora delegate statement changes response.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraDelegateStatementChangesResponse {
    /// Current page number.
    pub page: i64,
    /// Current page size.
    pub offset: i64,
    /// Returned change rows.
    pub changes: Vec<AgoraDelegateStatementChangeDto>,
    /// Total number of matching rows before pagination.
    pub count: i64,
}

/// Legacy Agora delegate statement change row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraDelegateStatementChangeDto {
    /// Delegate account id.
    pub account: String,
    /// Statement change timestamp, when present.
    pub created: Option<String>,
}

/// Legacy Agora voting power chart point.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct AgoraVotingPowerChartPoint {
    /// Block height coerced to a plain number.
    pub block_number: i64,
    /// Voting power as a string.
    pub vp: String,
    /// Timestamp in ISO-8601 format.
    pub timestamp: String,
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
    /// Telegram internal numeric channel identifier.
    pub channel_telegram_id: i64,
    /// Display name shown for the channel.
    pub name: String,
    /// Optional public username (`@...`) if configured.
    pub username: Option<String>,
    /// Telegram channel type (for example `channel`, `supergroup`, `private`).
    pub channel_type: String,
    /// Creation timestamp in RFC 3339.
    pub created_at: String,
    /// Last update timestamp in RFC 3339.
    pub updated_at: String,
}

/// Indexed Telegram user metadata.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramUserDto {
    /// Telegram internal numeric user identifier.
    pub user_telegram_id: i64,
    /// Optional public username (`@...`) for the user.
    pub username: Option<String>,
    /// Optional display name from Telegram profile.
    pub display_name: Option<String>,
    /// Creation timestamp in RFC 3339.
    pub created_at: String,
    /// Last update timestamp in RFC 3339.
    pub updated_at: String,
}

/// Indexed Telegram message row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramMessageDto {
    /// Telegram channel id this message belongs to.
    pub channel_telegram_id: i64,
    /// Telegram message id inside its channel.
    pub telegram_message_id: i64,
    /// Message type classification.
    pub message_type: String,
    /// Optional Telegram sender user id.
    pub user_telegram_id: Option<i64>,
    /// Optional sender username.
    pub from_username: Option<String>,
    /// Optional sender display name.
    pub from_display_name: Option<String>,
    /// Optional replied-to message id.
    pub reply_to_telegram_message_id: Option<i64>,
    /// Message timestamp in RFC 3339.
    pub date: String,
    /// Last edited timestamp in RFC 3339.
    pub edited_date: Option<String>,
    /// Message plain text body.
    pub text: Option<String>,
    /// Optional parsed text entities (mentions, links, formatting runs).
    pub text_entities: Option<serde_json::Value>,
    /// Whether this message includes media content.
    pub has_media: bool,
    /// Optional media subtype label.
    pub media_type: Option<String>,
    /// Optional media storage path or reference.
    pub media_path: Option<String>,
    /// Row creation timestamp in RFC 3339.
    pub created_at: String,
    /// Row update timestamp in RFC 3339.
    pub updated_at: String,
}

/// Indexed Telegram message reaction row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramReactionDto {
    /// Internal primary key for the reaction row.
    pub id: i64,
    /// Telegram channel id for the reacted message.
    pub channel_telegram_id: i64,
    /// Telegram message id this reaction applies to.
    pub telegram_message_id: i64,
    /// Telegram user id that added the reaction.
    pub user_telegram_id: Option<i64>,
    /// Optional reaction username.
    pub username: Option<String>,
    /// Optional reaction user display name.
    pub display_name: Option<String>,
    /// Emoji used as reaction.
    pub emoji: String,
    /// Reaction timestamp in RFC 3339.
    pub date: String,
    /// Row creation timestamp in RFC 3339.
    pub created_at: String,
}

/// Indexed Telegram service-action row.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct TelegramServiceActionDto {
    /// Internal primary key for the service action row.
    pub id: i64,
    /// Telegram channel id for the source message.
    pub channel_telegram_id: i64,
    /// Telegram message id that triggered this action.
    pub telegram_message_id: i64,
    /// Service action type.
    pub action: String,
    /// Telegram user id of the actor.
    pub actor_telegram_id: Option<i64>,
    /// Optional actor username.
    pub actor_username: Option<String>,
    /// Optional actor display name.
    pub actor_display_name: Option<String>,
    /// Optional service-action metadata.
    pub metadata: Option<serde_json::Value>,
    /// Row creation timestamp in RFC 3339.
    pub created_at: String,
}

/// Query options for listing indexed Telegram channels.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TelegramChannelsQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Substring match on channel `name` and `username`.
    ///
    /// Empty strings are ignored and treated as if no filter was supplied.
    pub q: Option<String>,
    /// Exact match on Telegram channel `type`.
    /// Common values include `channel`, `supergroup`, and `private`.
    pub channel_type: Option<String>,
    /// Substring match on channel `username`.
    ///
    /// Empty strings are ignored and treated as if no filter was supplied.
    pub username: Option<String>,
}

/// Query options for listing indexed Telegram users.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TelegramUsersQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Substring match on `username` and `display_name`.
    ///
    /// Empty strings are ignored and treated as if no filter was supplied.
    pub q: Option<String>,
}

/// Query options for listing indexed Telegram messages in one channel.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TelegramMessagesQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Substring match on message `text`.
    ///
    /// Empty strings are ignored and treated as if no filter was supplied.
    pub q: Option<String>,
    /// Exact match on Telegram author user id.
    ///
    /// If the user id is unknown in the channel messages index, an empty page is returned.
    pub user_telegram_id: Option<i64>,
    /// Filter by message type.
    ///
    /// Typical values are `message`, `media`, `service`, and `unknown`.
    pub message_type: Option<String>,
    /// Filter by whether the message contains media.
    pub has_media: Option<bool>,
    /// Lower bound for `date` (inclusive), RFC 3339 date-time string.
    ///
    /// Example: `2025-11-30T00:00:00Z`.
    pub date_after: Option<String>,
    /// Upper bound for `date` (inclusive), RFC 3339 date-time string.
    ///
    /// Example: `2025-11-30T23:59:59Z`.
    pub date_before: Option<String>,
}

/// Query options for listing indexed Telegram reactions for one message.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TelegramReactionsQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Filter by exact reaction emoji value.
    ///
    /// This matches the raw stored emoji glyph or short-code text.
    pub emoji: Option<String>,
    /// Exact match on Telegram reacting user id.
    pub user_telegram_id: Option<i64>,
}

/// Query options for listing indexed Telegram service actions for one message.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TelegramServiceActionsQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Exact match on service action type.
    pub action: Option<String>,
    /// Exact match on Telegram actor user id.
    pub actor_telegram_id: Option<i64>,
}

/// Query options for listing indexed Telegram messages authored by one user.
#[derive(Debug, Deserialize, IntoParams)]
pub struct TelegramUserMessagesQuery {
    /// Maximum number of rows to return in one response page.
    ///
    /// - Omit to use the default value of 20.
    /// - The API clamps values to 1..=100.
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    /// Opaque pagination cursor from a previous response's `next_cursor`.
    pub cursor: Option<String>,
    /// Optional exact channel filter by Telegram channel id.
    ///
    /// If the channel does not exist, this endpoint returns an empty result set.
    pub channel_telegram_id: Option<i64>,
    /// Substring match on message `text`.
    ///
    /// Empty strings are ignored and treated as if no filter was supplied.
    pub q: Option<String>,
    /// Filter by message type.
    ///
    /// Typical values are `message`, `media`, `service`, and `unknown`.
    pub message_type: Option<String>,
    /// Filter by whether the message contains media.
    pub has_media: Option<bool>,
    /// Lower bound for `date` (inclusive), RFC 3339 date-time string.
    ///
    /// Example: `2025-11-30T00:00:00Z`.
    pub date_after: Option<String>,
    /// Upper bound for `date` (inclusive), RFC 3339 date-time string.
    ///
    /// Example: `2025-11-30T23:59:59Z`.
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
