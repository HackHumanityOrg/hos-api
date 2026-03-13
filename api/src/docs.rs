//! OpenAPI documentation definition for the API crate

use crate::{errors, handlers, types};
use utoipa::OpenApi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "HOS API",
        version = "0.1.0",
        description = "Read-only REST API for indexed identity, Discourse, Telegram, and veNEAR governance data.\n\nOpenAPI contract notes for AI clients:\n- The API is read-only. Every endpoint is discoverable through generated OpenAPI metadata.\n- `/api/v1` list endpoints use `limit` and `cursor` with opaque base64url cursors.\n  - `limit` defaults to 20 and is clamped to a maximum of 100.\n  - `cursor` is an opaque base64url token from a prior response and should be treated as an opaque string.\n  - Omit `cursor` to request the first page in a sequence.\n  - Continue until `next_cursor` is `null` or absent.\n- Paginated `/api/v1` responses return `data`, `next_cursor`, and `has_more`.\n- Legacy `/api/agora` endpoints intentionally preserve page-based params and older payload shapes for compatibility.\n- `next_cursor` and pagination order are endpoint-specific; always follow the ordering declared in each endpoint description.\n- Timestamp fields use RFC 3339 / ISO-8601 format with timezone.\n- Error responses follow RFC 9457 Problem Details as `application/problem+json`, including machine-friendly `code` when present.\n- Indexers are asynchronous: newly emitted data may lag or reorder briefly during backfill.\n- `identity` values are stable while indexer-sourced entity references may vary by environment and snapshot time.",
        contact(
            name = "HOS API Team",
            url = "https://github.com/HackHumanityOrg/hos-api"
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
        handlers::venear::get_config,
        handlers::venear::list_config_history,
        handlers::venear::list_proposals,
        handlers::venear::get_proposal,
        handlers::venear::list_proposal_timeline,
        handlers::venear::list_proposal_votes,
        handlers::venear::list_proposal_vote_history,
        handlers::venear::list_accounts,
        handlers::venear::get_account,
        handlers::venear::list_account_proposals,
        handlers::venear::list_account_votes,
        handlers::venear::list_account_vote_history,
        handlers::venear::list_account_voting_power_history,
        handlers::venear::delegations::list_account_delegation_history,
        handlers::venear::lockups::list_account_lockup_events,
        handlers::venear::list_account_rewards_events,
        handlers::venear::list_delegations,
        handlers::venear::list_lockups,
        handlers::venear::get_lockup,
        handlers::venear::list_lockup_events,
        handlers::venear::list_admin_actions,
        handlers::venear::list_activity,
        handlers::agora::get_approved_proposals,
        handlers::agora::get_pending_proposals,
        handlers::agora::get_proposal_votes,
        handlers::agora::get_proposal_non_voters,
        handlers::agora::get_proposal_charts,
        handlers::agora::get_proposal_quorum,
        handlers::agora::get_delegates,
        handlers::agora::get_delegate,
        handlers::agora::get_delegate_voting_history,
        handlers::agora::get_delegate_delegated_from,
        handlers::agora::get_delegate_delegated_to,
        handlers::agora::get_delegate_hos_activity,
        handlers::agora::get_delegate_statement_changes,
        handlers::agora::get_voting_power_chart,
        handlers::agora::get_vote_changes,
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
        types::VenearTokenAmountDto,
        types::VenearChainRefDto,
        types::VenearProposalListResponse,
        types::VenearProposalTimelineListResponse,
        types::VenearProposalVoteListResponse,
        types::VenearProposalVoteHistoryListResponse,
        types::VenearAccountListResponse,
        types::VenearAccountProposalListResponse,
        types::VenearAccountVoteListResponse,
        types::VenearAccountVoteHistoryListResponse,
        types::VenearAccountVotingPowerHistoryListResponse,
        types::VenearAccountDelegationHistoryListResponse,
        types::VenearAccountLockupEventsListResponse,
        types::VenearAccountRewardsEventsListResponse,
        types::VenearDelegationListResponse,
        types::VenearLockupListResponse,
        types::VenearLockupDetailDto,
        types::VenearLockupEventsListResponse,
        types::VenearConfigResponse,
        types::VenearConfigHistoryListResponseV2,
        types::VenearAdminActionsListResponse,
        types::VenearActivityListResponse,
        types::VenearProposalSummaryV2Dto,
        types::VenearProposalDetailDto,
        types::VenearProposalMilestonesDto,
        types::VenearProposalSnapshotDto,
        types::VenearProposalTallyDto,
        types::VenearProposalTimelineEventDto,
        types::VenearProposalVoteV2Dto,
        types::VenearProposalVoteHistoryEventDto,
        types::VenearAccountSummaryDto,
        types::VenearAccountDetailDto,
        types::VenearAccountGovernanceStateDto,
        types::VenearAccountDelegationSummaryDto,
        types::VenearAccountLockupSummaryDto,
        types::VenearAccountParticipationDto,
        types::VenearAccountVoteDto,
        types::VenearAccountVotingPowerEventV2Dto,
        types::VenearAccountDelegationHistoryEventDto,
        types::VenearDelegationEdgeDto,
        types::VenearLockupSummaryDto,
        types::VenearLockupEventDto,
        types::VenearRewardEventDto,
        types::VenearConfigSnapshotDto,
        types::VenearConfigHistoryEntryV2Dto,
        types::VenearAdminActionDto,
        types::VenearActivityItemDto,
        types::AgoraLegacyErrorBody,
        types::AgoraLegacyMessageBody,
        types::AgoraProposalListResponse,
        types::AgoraProposalDto,
        types::AgoraProposalQuorumResponse,
        types::AgoraProposalVotesResponse,
        types::AgoraProposalVoteDto,
        types::AgoraProposalChartsResponse,
        types::AgoraProposalChartPointDto,
        types::AgoraProposalNonVotersResponse,
        types::AgoraProposalNonVoterDto,
        types::AgoraVoteChangesResponse,
        types::AgoraVoteChangeDto,
        types::AgoraVoteChangeEntryDto,
        types::AgoraDelegatesResponse,
        types::AgoraTopIssueDto,
        types::AgoraDelegateSummaryDto,
        types::AgoraDelegateDetailResponse,
        types::AgoraDelegateDetailDto,
        types::AgoraDelegateVotingHistoryResponse,
        types::AgoraDelegateVotingHistoryDto,
        types::AgoraDelegationEventsResponse,
        types::AgoraDelegationEventDto,
        types::AgoraHosActivityResponse,
        types::AgoraHosActivityDto,
        types::AgoraDelegateStatementChangesResponse,
        types::AgoraDelegateStatementChangeDto,
        types::AgoraVotingPowerChartPoint,
    )),
        tags(
        (name = "health", description = "Service and dependency health diagnostics"),
        (name = "identity", description = "Read endpoints over linked identity entities (persons and linked platform accounts)"),
        (name = "discourse", description = "Read endpoints over indexed Discourse entities (instances, categories, topics, posts, likes, users)"),
        (name = "discourse-ops", description = "Operational diagnostics for data integrity and coverage checks"),
        (name = "telegram", description = "Read endpoints over indexed Telegram entities (channels, users, messages, reactions, service actions)"),
        (name = "venear", description = "Read endpoints over indexed veNEAR governance entities (proposals, accounts, votes, delegations, lockups, config, admin actions, activity)"),
        (name = "agora", description = "Legacy Agora-compatible governance endpoints backed by indexed HOS materialized views")
    )
)]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::ApiDoc;
    use utoipa::OpenApi;

    const HTTP_METHODS: [&str; 7] = ["get", "post", "put", "delete", "patch", "head", "options"];

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
            info_description.contains("/api/v1` list endpoints use `limit` and `cursor`")
                && info_description.contains(
                    "Legacy `/api/agora` endpoints intentionally preserve page-based params"
                ),
            "api info should document canonical and Agora pagination conventions"
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

        let accounts_get = paths
            .get("/api/v1/venear/accounts")
            .and_then(|p| p.get("get"))
            .and_then(|op| op.as_object())
            .expect("accounts get operation");
        assert!(
            accounts_get
                .get("operationId")
                .and_then(|v| v.as_str())
                .expect("accounts operation id")
                .contains("VenearAccountsCanonical"),
            "accounts endpoint should expose the canonical veNEAR operation id"
        );

        let agora_delegate_get = paths
            .get("/api/agora/delegates/{address}")
            .and_then(|p| p.get("get"))
            .and_then(|op| op.as_object())
            .expect("agora delegate get operation");
        assert!(
            agora_delegate_get
                .get("description")
                .and_then(|v| v.as_str())
                .expect("agora delegate description")
                .contains("legacy Agora payload shape"),
            "agora delegate endpoint should document compatibility intent"
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

        let identity_person_schema = schemas
            .get("IdentityPersonDto")
            .and_then(|s| s.as_object())
            .expect("IdentityPersonDto schema");
        let identity_person_properties = identity_person_schema
            .get("properties")
            .and_then(|p| p.as_object())
            .expect("IdentityPersonDto properties");
        assert!(
            identity_person_properties.contains_key("name"),
            "identity person schema should expose name"
        );

        let discourse_instance_properties = schemas
            .get("DiscourseInstanceDto")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.as_object())
            .expect("DiscourseInstanceDto properties");
        assert!(
            discourse_instance_properties.contains_key("instance"),
            "discourse instance schema should expose instance"
        );
        assert!(
            !discourse_instance_properties.contains_key("key"),
            "legacy discourse instance key field should be removed"
        );

        let discourse_topic_properties = schemas
            .get("DiscourseTopicDto")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.as_object())
            .expect("DiscourseTopicDto properties");
        assert!(
            discourse_topic_properties.contains_key("topic_id"),
            "discourse topic schema should expose topic_id"
        );
        assert!(
            !discourse_topic_properties.contains_key("external_id"),
            "legacy discourse topic external_id field should be removed"
        );

        let telegram_channel_properties = schemas
            .get("TelegramChannelDto")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.as_object())
            .expect("TelegramChannelDto properties");
        assert!(
            telegram_channel_properties.contains_key("channel_telegram_id"),
            "telegram channel schema should expose channel_telegram_id"
        );
        assert!(
            !telegram_channel_properties.contains_key("telegram_id"),
            "legacy telegram_id field should be removed from channels"
        );

        let telegram_user_properties = schemas
            .get("TelegramUserDto")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.as_object())
            .expect("TelegramUserDto properties");
        assert!(
            telegram_user_properties.contains_key("user_telegram_id"),
            "telegram user schema should expose user_telegram_id"
        );
        assert!(
            !telegram_user_properties.contains_key("telegram_id"),
            "legacy telegram_id field should be removed from users"
        );

        let telegram_message_properties = schemas
            .get("TelegramMessageDto")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.as_object())
            .expect("TelegramMessageDto properties");
        assert!(
            telegram_message_properties.contains_key("user_telegram_id"),
            "telegram message schema should expose user_telegram_id"
        );
        assert!(
            !telegram_message_properties.contains_key("from_user_telegram_id"),
            "legacy from_user_telegram_id field should be removed"
        );

        let venear_voting_power_properties = schemas
            .get("VenearAccountVotingPowerEventV2Dto")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.as_object())
            .expect("VenearAccountVotingPowerEventV2Dto properties");
        assert!(
            venear_voting_power_properties.contains_key("account_id"),
            "account voting power events should expose scoped account_id"
        );

        let venear_delegation_properties = schemas
            .get("VenearAccountDelegationHistoryEventDto")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.as_object())
            .expect("VenearAccountDelegationHistoryEventDto properties");
        assert!(
            venear_delegation_properties.contains_key("account_id"),
            "delegation events should expose scoped account_id"
        );

        let agora_delegate_properties = schemas
            .get("AgoraDelegateDetailDto")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.as_object())
            .expect("AgoraDelegateDetailDto properties");
        assert!(
            agora_delegate_properties.contains_key("notificationPreferences"),
            "Agora delegate detail should expose legacy notificationPreferences"
        );
        assert!(
            agora_delegate_properties.contains_key("delegatedFromCount"),
            "Agora delegate detail should expose delegatedFromCount"
        );

        let telegram_messages_parameters = paths
            .get("/api/v1/telegram/channels/{channel_telegram_id}/messages")
            .and_then(|path| path.get("get"))
            .and_then(|op| op.get("parameters"))
            .and_then(|params| params.as_array())
            .expect("telegram messages parameters");
        assert!(
            telegram_messages_parameters
                .iter()
                .any(|param| param.get("name").and_then(|v| v.as_str()) == Some("user_telegram_id")),
            "telegram messages query should expose user_telegram_id"
        );
        assert!(
            !telegram_messages_parameters.iter().any(|param| {
                param.get("name").and_then(|v| v.as_str()) == Some("from_user_telegram_id")
            }),
            "legacy from_user_telegram_id query parameter should be removed"
        );
    }

    #[test]
    fn openapi_has_complete_operation_metadata_for_ai_exploration() {
        let doc = ApiDoc::openapi();
        let value = serde_json::to_value(&doc).expect("serialize openapi");

        let paths = value
            .get("paths")
            .and_then(|v| v.as_object())
            .expect("paths object");
        assert!(
            !paths.is_empty(),
            "openapi specification must expose at least one path"
        );

        for (path, path_item) in paths {
            let path_item = path_item
                .as_object()
                .expect("path item should be an object");
            let path_parameters = path_item
                .get("parameters")
                .and_then(|v| v.as_array())
                .map_or(&[] as &[serde_json::Value], |v| v.as_slice());

            for method in HTTP_METHODS {
                let operation = match path_item.get(method).and_then(|v| v.as_object()) {
                    Some(operation) => operation,
                    None => continue,
                };

                let summary = operation.get("summary").and_then(|v| v.as_str());
                assert!(
                    summary.is_some_and(|value| !value.trim().is_empty()),
                    "operation summary missing for {} {}",
                    method,
                    path
                );

                let description = operation.get("description").and_then(|v| v.as_str());
                assert!(
                    description.is_some_and(|value| !value.trim().is_empty()),
                    "operation description missing for {} {}",
                    method,
                    path
                );

                let operation_id = operation.get("operationId").and_then(|v| v.as_str());
                assert!(
                    operation_id.is_some_and(|value| !value.trim().is_empty()),
                    "operationId missing for {} {}",
                    method,
                    path
                );

                let responses = operation
                    .get("responses")
                    .and_then(|v| v.as_object())
                    .expect("operation responses");
                assert!(
                    !responses.is_empty(),
                    "operation responses missing for {} {}",
                    method,
                    path
                );
                for (status, response) in responses {
                    if response.get("$ref").is_some() {
                        continue;
                    }
                    let response = response.as_object().unwrap_or_else(|| {
                        panic!(
                            "response should be object for {} {} {}",
                            method, path, status
                        )
                    });
                    let response_description = response
                        .get("description")
                        .and_then(|v| v.as_str())
                        .expect("response description");
                    assert!(
                        !response_description.trim().is_empty(),
                        "response description missing for {} {} status {}",
                        method,
                        path,
                        status
                    );
                }

                let operation_parameters = operation
                    .get("parameters")
                    .and_then(|v| v.as_array())
                    .map_or(&[] as &[serde_json::Value], |v| v.as_slice());

                for parameter in path_parameters.iter().chain(operation_parameters.iter()) {
                    let parameter = match parameter.as_object() {
                        Some(parameter) => parameter,
                        None => continue,
                    };
                    if parameter.get("$ref").is_some() {
                        continue;
                    }

                    let description = parameter
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let name = parameter
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("<unnamed>");
                    if name == "page" {
                        // Utoipa currently surfaces flattened query helpers as a synthetic
                        // wrapper parameter even though the runtime contract exposes the
                        // flattened inner fields instead.
                        continue;
                    }
                    assert!(
                        !description.trim().is_empty(),
                        "parameter description missing for {} {} ({})",
                        method,
                        path,
                        name
                    );
                }
            }
        }
    }
}
