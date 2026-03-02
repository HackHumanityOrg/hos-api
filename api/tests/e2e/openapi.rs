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
        paths.contains_key("/api/v1/discourse/instances/{instance}/sync-lag"),
        "sync lag path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/proposals"),
        "venear proposals path present"
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
        paths.contains_key("/api/v1/venear/lockups/{lockup_account_id}/actions"),
        "venear lockup actions path present"
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

    let venear_lockups_operation = paths
        .get("/api/v1/venear/lockups")
        .and_then(|path| path.get("get"))
        .and_then(|op| op.as_object())
        .expect("venear lockups get operation");
    assert!(
        venear_lockups_operation
            .get("description")
            .and_then(|v| v.as_str())
            .expect("lockups description")
            .contains("lifecycle metadata"),
        "lockups description should explain dataset semantics"
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

    let schemas = value
        .get("components")
        .and_then(|components| components.get("schemas"))
        .and_then(|schemas| schemas.as_object())
        .expect("schemas listing");
    assert!(schemas.contains_key("HealthResponse"));
    assert!(schemas.contains_key("DatabaseHealth"));
    assert!(schemas.contains_key("ApiError"));
    assert!(schemas.contains_key("DiscourseTopicDto"));
    assert!(schemas.contains_key("SyncLagResponse"));
    assert!(schemas.contains_key("TelegramChannelDto"));
    assert!(schemas.contains_key("TelegramMessageDto"));
    assert!(schemas.contains_key("TelegramReactionDto"));
    assert!(schemas.contains_key("TelegramServiceActionDto"));
    assert!(schemas.contains_key("VenearProposalDto"));
    assert!(schemas.contains_key("VenearVoteDto"));
    assert!(schemas.contains_key("VenearLockupActionDto"));

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
