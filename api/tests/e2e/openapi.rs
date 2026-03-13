use api::ApiDoc;
use utoipa::OpenApi;

#[test]
fn openapi_spec_exposes_health_path_and_schemas() {
    let doc = ApiDoc::openapi();
    let value = serde_json::to_value(&doc).expect("serialize openapi");

    let info = value
        .get("info")
        .and_then(|v| v.as_object())
        .expect("openapi info object");
    let description = info
        .get("description")
        .and_then(|v| v.as_str())
        .expect("api description present");
    assert!(
        description.contains("/api/v1` list endpoints use `limit` and `cursor`")
            && description
                .contains("Legacy `/api/agora` endpoints intentionally preserve page-based params"),
        "api description should document both canonical and Agora pagination conventions"
    );

    let paths = value
        .get("paths")
        .expect("paths section")
        .as_object()
        .expect("paths object");
    assert!(paths.contains_key("/health"), "health path present");
    assert!(
        paths.contains_key("/api/v1/discourse/instances"),
        "discourse instances path present"
    );
    assert!(
        paths.contains_key("/api/v1/identity/persons"),
        "identity persons path present"
    );
    assert!(
        paths.contains_key("/api/v1/identity/persons/{person_id}/accounts"),
        "identity person accounts path present"
    );
    assert!(
        paths.contains_key("/api/v1/discourse/instances/{instance}/sync-lag"),
        "sync lag path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/proposals"),
        "venear proposals path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/config"),
        "venear config path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/config/history"),
        "venear config history path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/proposals/{proposal_id}"),
        "venear proposal details path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/proposals/{proposal_id}/timeline"),
        "venear proposal timeline path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/proposals/{proposal_id}/vote_history"),
        "venear proposal vote history path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/accounts"),
        "venear accounts path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/accounts/{account_id}"),
        "venear account details path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/accounts/{account_id}/voting_power_history"),
        "venear account voting power history path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/delegations"),
        "venear delegations path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/lockups"),
        "venear lockups path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/admin/actions"),
        "venear admin actions path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/activity"),
        "venear activity path present"
    );
    assert!(
        paths.contains_key("/api/v1/telegram/channels"),
        "telegram channels path present"
    );
    assert!(
        paths.contains_key("/api/v1/telegram/channels/{channel_telegram_id}/messages/{telegram_message_id}/reactions"),
        "telegram reactions path present"
    );
    assert!(
        paths.contains_key("/api/v1/telegram/users/{user_telegram_id}/messages"),
        "telegram user messages path present"
    );
    assert!(
        paths.contains_key("/api/agora/proposal/approved"),
        "agora approved proposals path present"
    );
    assert!(
        paths.contains_key("/api/agora/proposal/{proposal_id}/quorum"),
        "agora proposal quorum path present"
    );
    assert!(
        paths.contains_key("/api/agora/delegates/{address}"),
        "agora delegate detail path present"
    );
    assert!(
        paths.contains_key("/api/agora/get_voting_power_chart/{account_id}"),
        "agora voting power chart path present"
    );
    assert!(
        paths.contains_key("/api/agora/vote_changes/{proposal_id}"),
        "agora vote changes path present"
    );

    let health_operation = paths
        .get("/health")
        .and_then(|path| path.get("get"))
        .and_then(|op| op.as_object())
        .expect("health get operation");
    assert_eq!(
        health_operation
            .get("summary")
            .and_then(|v| v.as_str())
            .expect("health summary"),
        "Get API health status"
    );
    assert!(
        health_operation
            .get("description")
            .and_then(|v| v.as_str())
            .expect("health description")
            .contains("database probe"),
        "health description should explain the database probe"
    );

    let discourse_topics_operation = paths
        .get("/api/v1/discourse/instances/{instance}/topics")
        .and_then(|path| path.get("get"))
        .and_then(|op| op.as_object())
        .expect("discourse topics get operation");
    assert!(
        discourse_topics_operation
            .get("description")
            .and_then(|v| v.as_str())
            .expect("topics description")
            .contains("bumped_at"),
        "topics description should explain ordering semantics"
    );

    let venear_accounts_operation = paths
        .get("/api/v1/venear/accounts")
        .and_then(|path| path.get("get"))
        .and_then(|op| op.as_object())
        .expect("venear accounts get operation");
    assert!(
        venear_accounts_operation
            .get("operationId")
            .and_then(|v| v.as_str())
            .expect("accounts operation id")
            .contains("VenearAccountsCanonical"),
        "venear accounts operation id should expose canonical surface"
    );

    let agora_delegate_operation = paths
        .get("/api/agora/delegates/{address}")
        .and_then(|path| path.get("get"))
        .and_then(|op| op.as_object())
        .expect("agora delegate get operation");
    assert_eq!(
        agora_delegate_operation
            .get("tags")
            .and_then(|v| v.as_array())
            .and_then(|tags| tags.first())
            .and_then(|v| v.as_str()),
        Some("agora")
    );

    let telegram_messages_operation = paths
        .get("/api/v1/telegram/channels/{channel_telegram_id}/messages")
        .and_then(|path| path.get("get"))
        .and_then(|op| op.as_object())
        .expect("telegram messages get operation");
    assert!(
        telegram_messages_operation
            .get("description")
            .and_then(|v| v.as_str())
            .expect("telegram messages description")
            .contains("cursor pagination"),
        "telegram messages description should explain pagination semantics"
    );

    let topics_parameters = paths
        .get("/api/v1/discourse/instances/{instance}/topics")
        .and_then(|path| path.get("get"))
        .and_then(|op| op.get("parameters"))
        .and_then(|params| params.as_array())
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

    let schemas = value
        .get("components")
        .and_then(|components| components.get("schemas"))
        .and_then(|schemas| schemas.as_object())
        .expect("schemas listing");
    assert!(schemas.contains_key("HealthResponse"));
    assert!(schemas.contains_key("DatabaseHealth"));
    assert!(schemas.contains_key("ApiError"));
    assert!(schemas.contains_key("IdentityPersonsListResponse"));
    assert!(schemas.contains_key("IdentityPersonDto"));
    assert!(schemas.contains_key("IdentityAccountDto"));
    assert!(schemas.contains_key("IdentityPersonAccountsResponse"));
    assert!(schemas.contains_key("DiscourseTopicDto"));
    assert!(schemas.contains_key("SyncLagResponse"));
    assert!(schemas.contains_key("TelegramChannelDto"));
    assert!(schemas.contains_key("TelegramMessageDto"));
    assert!(schemas.contains_key("TelegramReactionDto"));
    assert!(schemas.contains_key("TelegramServiceActionDto"));
    assert!(schemas.contains_key("VenearProposalSummaryV2Dto"));
    assert!(schemas.contains_key("VenearProposalDetailDto"));
    assert!(schemas.contains_key("VenearProposalVoteV2Dto"));
    assert!(schemas.contains_key("VenearConfigResponse"));
    assert!(schemas.contains_key("VenearConfigHistoryListResponseV2"));
    assert!(schemas.contains_key("VenearConfigSnapshotDto"));
    assert!(schemas.contains_key("VenearAccountSummaryDto"));
    assert!(schemas.contains_key("VenearAccountVotingPowerEventV2Dto"));
    assert!(schemas.contains_key("AgoraProposalDto"));
    assert!(schemas.contains_key("AgoraDelegateDetailDto"));
    assert!(schemas.contains_key("AgoraVoteChangesResponse"));
    assert!(schemas.contains_key("AgoraHosActivityDto"));

    let discourse_instance_props = schemas
        .get("DiscourseInstanceDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("DiscourseInstanceDto properties");
    assert!(
        discourse_instance_props.contains_key("instance"),
        "instance payload should expose instance identifier"
    );
    assert!(
        !discourse_instance_props.contains_key("key"),
        "legacy key field should be removed"
    );

    let discourse_category_props = schemas
        .get("DiscourseCategoryDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("DiscourseCategoryDto properties");
    assert!(discourse_category_props.contains_key("category_id"));
    assert!(!discourse_category_props.contains_key("external_id"));

    let discourse_topic_props = schemas
        .get("DiscourseTopicDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("DiscourseTopicDto properties");
    assert!(discourse_topic_props.contains_key("topic_id"));
    assert!(!discourse_topic_props.contains_key("external_id"));

    let discourse_post_props = schemas
        .get("DiscoursePostDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("DiscoursePostDto properties");
    assert!(discourse_post_props.contains_key("post_id"));
    assert!(!discourse_post_props.contains_key("external_id"));

    let discourse_user_props = schemas
        .get("DiscourseUserDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("DiscourseUserDto properties");
    assert!(discourse_user_props.contains_key("user_id"));
    assert!(!discourse_user_props.contains_key("external_id"));

    let discourse_like_props = schemas
        .get("DiscoursePostLikeDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("DiscoursePostLikeDto properties");
    assert!(discourse_like_props.contains_key("post_id"));
    assert!(discourse_like_props.contains_key("user_id"));
    assert!(!discourse_like_props.contains_key("external_post_id"));
    assert!(!discourse_like_props.contains_key("external_user_id"));

    let discourse_revision_props = schemas
        .get("DiscoursePostRevisionDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("DiscoursePostRevisionDto properties");
    assert!(discourse_revision_props.contains_key("post_id"));
    assert!(!discourse_revision_props.contains_key("external_post_id"));

    let telegram_channel_props = schemas
        .get("TelegramChannelDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("TelegramChannelDto properties");
    assert!(telegram_channel_props.contains_key("channel_telegram_id"));
    assert!(!telegram_channel_props.contains_key("telegram_id"));

    let telegram_user_props = schemas
        .get("TelegramUserDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("TelegramUserDto properties");
    assert!(telegram_user_props.contains_key("user_telegram_id"));
    assert!(!telegram_user_props.contains_key("telegram_id"));

    let telegram_message_props = schemas
        .get("TelegramMessageDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("TelegramMessageDto properties");
    assert!(telegram_message_props.contains_key("user_telegram_id"));
    assert!(!telegram_message_props.contains_key("from_user_telegram_id"));

    let venear_vp_props = schemas
        .get("VenearAccountVotingPowerEventV2Dto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("VenearAccountVotingPowerEventV2Dto properties");
    assert!(venear_vp_props.contains_key("account_id"));

    let venear_delegation_props = schemas
        .get("VenearAccountDelegationHistoryEventDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("VenearAccountDelegationHistoryEventDto properties");
    assert!(venear_delegation_props.contains_key("account_id"));

    let agora_delegate_props = schemas
        .get("AgoraDelegateDetailDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("AgoraDelegateDetailDto properties");
    assert!(agora_delegate_props.contains_key("notificationPreferences"));
    assert!(agora_delegate_props.contains_key("delegatedFromCount"));

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

    let cursor_meta = schemas
        .get("CursorPageMeta")
        .and_then(|schema| schema.as_object())
        .expect("cursor page meta schema");
    assert!(
        cursor_meta
            .get("description")
            .and_then(|v| v.as_str())
            .expect("cursor page meta description")
            .contains("Cursor pagination metadata"),
        "cursor metadata schema should be documented"
    );
}
