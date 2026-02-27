//! OpenAPI documentation definition for the API crate

use crate::{errors, handlers, types};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Data Commons API",
        version = "0.1.0",
        description = "REST API for the Data Commons platform",
        license(name = "MIT", url = "https://opensource.org/licenses/MIT")
    ),
    paths(
        handlers::health::health_check,
        handlers::discourse::list_instances,
        handlers::discourse::get_instance,
        handlers::discourse::list_categories,
        handlers::discourse::get_category,
        handlers::discourse::list_topics,
        handlers::discourse::get_topic,
        handlers::discourse::list_topic_posts,
        handlers::discourse::get_post,
        handlers::discourse::list_post_likes,
        handlers::discourse::list_post_revisions,
        handlers::discourse::list_users,
        handlers::discourse::get_user,
        handlers::discourse::list_user_posts,
        handlers::discourse::get_sync_lag,
        handlers::venear::list_proposals,
        handlers::venear::list_proposal_events,
        handlers::venear::list_proposal_votes,
        handlers::venear::list_account_balances,
        handlers::venear::list_lockups,
        handlers::venear::list_lockup_actions,
    ),
    components(schemas(
        types::HealthResponse,
        types::DatabaseHealth,
        errors::ApiError,
        types::CursorPageMeta,
        types::InstancesListResponse,
        types::CategoriesListResponse,
        types::TopicsListResponse,
        types::PostsListResponse,
        types::UsersListResponse,
        types::PostLikesListResponse,
        types::PostRevisionsListResponse,
        types::DiscourseInstanceDto,
        types::DiscourseCategoryDto,
        types::DiscourseTopicDto,
        types::DiscoursePostDto,
        types::DiscourseUserDto,
        types::DiscoursePostLikeDto,
        types::DiscoursePostRevisionDto,
        types::SyncLagSummary,
        types::SyncLagCheck,
        types::SyncLagResponse,
        types::VenearProposalsListResponse,
        types::VenearProposalEventsListResponse,
        types::VenearVotesListResponse,
        types::VenearAccountBalancesListResponse,
        types::VenearLockupsListResponse,
        types::VenearLockupActionsListResponse,
        types::VenearProposalDto,
        types::VenearProposalEventDto,
        types::VenearVoteDto,
        types::VenearAccountBalanceDto,
        types::VenearLockupDto,
        types::VenearLockupActionDto,
    )),
    tags(
        (name = "health", description = "Health check endpoints"),
        (name = "discourse", description = "Discourse read endpoints"),
        (name = "discourse-ops", description = "Discourse operational diagnostics"),
        (name = "venear", description = "veNEAR governance read endpoints")
    )
)]
pub struct ApiDoc;
