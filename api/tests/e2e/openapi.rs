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
        description.contains("Cursor pagination uses opaque base64url cursors"),
        "api description should document pagination conventions"
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
        paths.contains_key("/api/v1/venear/proposals/{proposal_id}"),
        "venear proposal details path present"
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
        paths.contains_key("/api/v1/venear/proposals/{proposal_id}/events"),
        "venear proposal events path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/delegates"),
        "venear delegates path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/delegates/{account_id}/voting-power"),
        "venear delegate voting power path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/delegates/{account_id}/delegations"),
        "venear delegate delegations path present"
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

    let venear_delegates_operation = paths
        .get("/api/v1/venear/delegates")
        .and_then(|path| path.get("get"))
        .and_then(|op| op.as_object())
        .expect("venear delegates get operation");
    assert!(
        venear_delegates_operation
            .get("description")
            .and_then(|v| v.as_str())
            .expect("delegates description")
            .contains("possible voters"),
        "delegates description should explain dataset semantics"
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
    assert!(schemas.contains_key("VenearProposalSummaryDto"));
    assert!(schemas.contains_key("VenearProposalDto"));
    assert!(schemas.contains_key("VenearVoteDto"));
    assert!(schemas.contains_key("VenearDelegateDto"));
    assert!(schemas.contains_key("VenearDelegateVotingPowerEventDto"));

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
        .get("VenearDelegateVotingPowerEventDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("VenearDelegateVotingPowerEventDto properties");
    assert!(venear_vp_props.contains_key("account_id"));

    let venear_delegation_props = schemas
        .get("VenearDelegationEventDto")
        .and_then(|schema| schema.get("properties"))
        .and_then(|properties| properties.as_object())
        .expect("VenearDelegationEventDto properties");
    assert!(venear_delegation_props.contains_key("account_id"));

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
