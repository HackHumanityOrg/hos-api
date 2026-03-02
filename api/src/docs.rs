//! OpenAPI documentation definition for the API crate

use crate::{errors, handlers, types};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Data Commons API",
        version = "0.1.0",
        description = "Read-only REST API for indexed identity, Discourse, Telegram, and veNEAR governance data.\n\nConventions:\n- Cursor pagination uses opaque base64url cursors.\n- List endpoints return `data`, `next_cursor`, and `has_more`.\n- Errors follow RFC 9457 Problem Details as `application/problem+json`.",
        contact(
            name = "Data Commons Team",
            url = "https://github.com/HackHumanityOrg/data-commons"
        ),
        license(name = "MIT", url = "https://opensource.org/licenses/MIT")
    ),
    servers((url = "/", description = "Current deployment base URL")),
    paths(
        handlers::health::health_check,
        handlers::identity::list_persons,
        handlers::identity::list_person_accounts,
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
        handlers::telegram::list_channels,
        handlers::telegram::get_channel,
        handlers::telegram::list_channel_messages,
        handlers::telegram::get_channel_message,
        handlers::telegram::list_message_reactions,
        handlers::telegram::list_message_service_actions,
        handlers::telegram::list_users,
        handlers::telegram::get_user,
        handlers::telegram::list_user_messages,
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
        types::IdentityPersonsListResponse,
        types::IdentityPersonDto,
        types::IdentityAccountDto,
        types::IdentityPersonAccountsResponse,
        types::TelegramChannelsListResponse,
        types::TelegramUsersListResponse,
        types::TelegramMessagesListResponse,
        types::TelegramReactionsListResponse,
        types::TelegramServiceActionsListResponse,
        types::TelegramChannelDto,
        types::TelegramUserDto,
        types::TelegramMessageDto,
        types::TelegramReactionDto,
        types::TelegramServiceActionDto,
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
        (name = "identity", description = "Read endpoints over linked identity entities (persons and linked platform accounts)"),
        (name = "discourse", description = "Read endpoints over indexed Discourse entities (instances, categories, topics, posts, likes, users)"),
        (name = "discourse-ops", description = "Operational diagnostics for data integrity and coverage checks"),
        (name = "telegram", description = "Read endpoints over indexed Telegram entities (channels, users, messages, reactions, service actions)"),
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

        let topics_parameters = topics_get
            .get("parameters")
            .and_then(|v| v.as_array())
            .expect("topics parameters");
        let topics_limit = topics_parameters
            .iter()
            .find(|param| param.get("name").and_then(|v| v.as_str()) == Some("limit"))
            .expect("topics limit parameter");
        assert_eq!(
            topics_limit
                .get("schema")
                .and_then(|schema| schema.get("default"))
                .and_then(|v| v.as_i64()),
            Some(20),
            "limit should be the only prefilled query value"
        );
        let topics_query = topics_parameters
            .iter()
            .find(|param| param.get("name").and_then(|v| v.as_str()) == Some("q"))
            .expect("topics q parameter");
        assert!(
            topics_query.get("example").is_none(),
            "search query should not be prefilled"
        );
        let topics_cursor = topics_parameters
            .iter()
            .find(|param| param.get("name").and_then(|v| v.as_str()) == Some("cursor"))
            .expect("topics cursor parameter");
        assert!(
            topics_cursor.get("example").is_none(),
            "cursor should not be prefilled"
        );

        let identity_persons_get = paths
            .get("/api/v1/identity/persons")
            .and_then(|p| p.get("get"))
            .and_then(|op| op.as_object())
            .expect("identity persons get operation");
        assert!(
            identity_persons_get
                .get("description")
                .and_then(|v| v.as_str())
                .expect("identity persons description")
                .contains("cursor pagination"),
            "identity persons endpoint should explain pagination semantics"
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

        let telegram_messages_get = paths
            .get("/api/v1/telegram/channels/{channel_telegram_id}/messages")
            .and_then(|p| p.get("get"))
            .and_then(|op| op.as_object())
            .expect("telegram messages get operation");
        assert!(
            telegram_messages_get
                .get("description")
                .and_then(|v| v.as_str())
                .expect("telegram messages description")
                .contains("cursor pagination"),
            "telegram messages endpoint should explain pagination semantics"
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

        assert!(
            schemas.contains_key("IdentityPersonsListResponse"),
            "identity persons response schema should be present"
        );
        assert!(
            schemas.contains_key("IdentityPersonDto"),
            "identity person schema should be present"
        );
        assert!(
            schemas.contains_key("IdentityAccountDto"),
            "identity account schema should be present"
        );
        assert!(
            schemas.contains_key("IdentityPersonAccountsResponse"),
            "identity person accounts schema should be present"
        );
    }
}
