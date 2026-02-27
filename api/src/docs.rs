//! OpenAPI documentation definition for the API crate

use crate::{errors, handlers, types};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Data Commons API",
        version = "0.1.0",
        description = "Read-only REST API for indexed Discourse and veNEAR governance data.\n\nConventions:\n- Cursor pagination uses opaque base64url cursors.\n- List endpoints return `data`, `next_cursor`, and `has_more`.\n- Errors follow RFC 9457 Problem Details as `application/problem+json`.",
        contact(
            name = "Data Commons Team",
            url = "https://github.com/HackHumanityOrg/data-commons"
        ),
        license(name = "MIT", url = "https://opensource.org/licenses/MIT")
    ),
    servers((url = "/", description = "Current deployment base URL")),
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
        (name = "health", description = "Service and dependency health diagnostics"),
        (name = "discourse", description = "Read endpoints over indexed Discourse entities (instances, categories, topics, posts, likes, users)"),
        (name = "discourse-ops", description = "Operational diagnostics for data integrity and coverage checks"),
        (name = "venear", description = "Read endpoints over indexed veNEAR governance entities (proposals, votes, balances, lockups, actions)")
    )
)]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::ApiDoc;
    use utoipa::OpenApi;

    #[test]
    fn openapi_has_descriptive_docs_for_core_endpoints() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(&doc).expect("serialize openapi");

        let info = value
            .get("info")
            .and_then(|v| v.as_object())
            .expect("info object");
        let info_description = info
            .get("description")
            .and_then(|v| v.as_str())
            .expect("api info description");
        assert!(
            info_description.contains("Cursor pagination uses opaque base64url cursors"),
            "api info should document pagination convention"
        );

        let paths = value
            .get("paths")
            .and_then(|v| v.as_object())
            .expect("paths object");

        let health_get = paths
            .get("/health")
            .and_then(|p| p.get("get"))
            .and_then(|op| op.as_object())
            .expect("health get operation");
        assert_eq!(
            health_get
                .get("summary")
                .and_then(|v| v.as_str())
                .expect("health summary"),
            "Get API health status"
        );

        let topics_get = paths
            .get("/api/v1/discourse/instances/{instance}/topics")
            .and_then(|p| p.get("get"))
            .and_then(|op| op.as_object())
            .expect("topics get operation");
        assert!(
            topics_get
                .get("description")
                .and_then(|v| v.as_str())
                .expect("topics description")
                .contains("bumped_at"),
            "topics endpoint should explain sorting semantics"
        );

        let lockups_get = paths
            .get("/api/v1/venear/lockups")
            .and_then(|p| p.get("get"))
            .and_then(|op| op.as_object())
            .expect("lockups get operation");
        assert!(
            lockups_get
                .get("description")
                .and_then(|v| v.as_str())
                .expect("lockups description")
                .contains("lifecycle metadata"),
            "lockups endpoint should explain dataset semantics"
        );

        let schemas = value
            .get("components")
            .and_then(|v| v.get("schemas"))
            .and_then(|v| v.as_object())
            .expect("schema object");

        let cursor_meta = schemas
            .get("CursorPageMeta")
            .and_then(|s| s.as_object())
            .expect("CursorPageMeta schema");
        assert!(
            cursor_meta
                .get("description")
                .and_then(|v| v.as_str())
                .expect("CursorPageMeta description")
                .contains("Cursor pagination metadata"),
            "pagination metadata schema should be documented"
        );
    }
}
