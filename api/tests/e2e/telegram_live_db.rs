use api::types::{
    TelegramChannelsListResponse, TelegramMessagesListResponse, TelegramReactionsListResponse,
    TelegramServiceActionsListResponse, TelegramUsersListResponse,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::de::DeserializeOwned;
use tower::ServiceExt;

#[path = "../support/mod.rs"]
mod support;

#[tokio::test]
async fn telegram_endpoints_return_seeded_data() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;
    seed_telegram_data(database.db().as_ref())
        .await
        .expect("seed telegram data");

    let channels_page_1: TelegramChannelsListResponse =
        get_json(&router, "/api/v1/telegram/channels?limit=1").await;
    assert_eq!(channels_page_1.data.len(), 1);
    assert!(channels_page_1.has_more);
    assert_eq!(channels_page_1.data[0].channel_telegram_id, -1005000000002);
    let cursor = channels_page_1.next_cursor.clone().expect("next cursor");

    let channels_page_2: TelegramChannelsListResponse =
        get_json(&router, &format!("/api/v1/telegram/channels?cursor={cursor}")).await;
    assert_eq!(channels_page_2.data.len(), 1);
    assert!(!channels_page_2.has_more);
    assert_eq!(channels_page_2.data[0].channel_telegram_id, -1005000000001);

    let channel_messages: TelegramMessagesListResponse = get_json(
        &router,
        "/api/v1/telegram/channels/-1005000000001/messages?q=proposal",
    )
    .await;
    assert_eq!(channel_messages.data.len(), 1);
    assert_eq!(channel_messages.data[0].telegram_message_id, 11);
    assert_eq!(channel_messages.data[0].user_telegram_id, Some(100002));
    assert_eq!(
        channel_messages.data[0].reply_to_telegram_message_id,
        Some(10)
    );

    let reactions: TelegramReactionsListResponse = get_json(
        &router,
        "/api/v1/telegram/channels/-1005000000001/messages/11/reactions",
    )
    .await;
    assert_eq!(reactions.data.len(), 2);
    assert_eq!(reactions.data[0].emoji, "fire");
    assert_eq!(reactions.data[0].user_telegram_id, Some(100002));

    let service_actions: TelegramServiceActionsListResponse = get_json(
        &router,
        "/api/v1/telegram/channels/-1005000000002/messages/20/service-actions",
    )
    .await;
    assert_eq!(service_actions.data.len(), 1);
    assert_eq!(service_actions.data[0].action, "message_pinned");
    assert_eq!(service_actions.data[0].actor_telegram_id, Some(100002));

    let users: TelegramUsersListResponse = get_json(&router, "/api/v1/telegram/users?q=ali").await;
    assert_eq!(users.data.len(), 1);
    assert_eq!(users.data[0].user_telegram_id, 100001);

    let user_messages: TelegramMessagesListResponse =
        get_json(&router, "/api/v1/telegram/users/100002/messages").await;
    assert_eq!(user_messages.data.len(), 1);
    assert_eq!(user_messages.data[0].telegram_message_id, 11);
    assert_eq!(user_messages.data[0].channel_telegram_id, -1005000000001);
}

async fn get_json<T: DeserializeOwned>(router: &Router, path: &str) -> T {
    let response = router
        .clone()
        .into_service()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    serde_json::from_slice(&bytes).expect("json body")
}

async fn seed_telegram_data(db: &impl ConnectionTrait) -> anyhow::Result<()> {
    execute_sql(
        db,
        r#"
        INSERT INTO telegram.telegram_channels
            (telegram_id, name, username, type, created_at, updated_at)
        VALUES
            (-1005000000001, 'Governance Hub', 'gov_hub', 'channel', NOW(), NOW()),
            (-1005000000002, 'Builders Chat', 'builders_chat', 'group', NOW(), NOW())
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO telegram.telegram_users
            (telegram_id, username, display_name, created_at, updated_at)
        VALUES
            (100001, 'alice', 'Alice', NOW(), NOW()),
            (100002, 'bob', 'Bob', NOW(), NOW())
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO telegram.telegram_messages
            (
                channel_id,
                telegram_message_id,
                message_type,
                from_user_id,
                reply_to_message_id,
                reply_to_telegram_message_id,
                date,
                edited_date,
                text,
                text_entities,
                has_media,
                media_type,
                media_path,
                created_at,
                updated_at
            )
        VALUES
            (
                (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000001),
                10,
                'message',
                (SELECT id FROM telegram.telegram_users WHERE telegram_id = 100001),
                NULL,
                NULL,
                '2026-02-01T10:00:00Z',
                NULL,
                'hello governance',
                NULL,
                FALSE,
                NULL,
                NULL,
                NOW(),
                NOW()
            )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO telegram.telegram_messages
            (
                channel_id,
                telegram_message_id,
                message_type,
                from_user_id,
                reply_to_message_id,
                reply_to_telegram_message_id,
                date,
                edited_date,
                text,
                text_entities,
                has_media,
                media_type,
                media_path,
                created_at,
                updated_at
            )
        VALUES
            (
                (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000001),
                11,
                'media',
                (SELECT id FROM telegram.telegram_users WHERE telegram_id = 100002),
                (
                    SELECT id
                    FROM telegram.telegram_messages
                    WHERE channel_id = (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000001)
                      AND telegram_message_id = 10
                ),
                10,
                '2026-02-01T10:05:00Z',
                '2026-02-01T10:06:00Z',
                'proposal draft',
                '[{"type":"bold","offset":0,"length":8}]'::jsonb,
                TRUE,
                'photo',
                NULL,
                NOW(),
                NOW()
            ),
            (
                (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000002),
                20,
                'service',
                NULL,
                NULL,
                NULL,
                '2026-02-01T11:00:00Z',
                NULL,
                NULL,
                NULL,
                FALSE,
                NULL,
                NULL,
                NOW(),
                NOW()
            )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO telegram.telegram_reactions
            (message_id, user_id, emoji, date, created_at)
        VALUES
            (
                (
                    SELECT id
                    FROM telegram.telegram_messages
                    WHERE channel_id = (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000001)
                      AND telegram_message_id = 11
                ),
                (SELECT id FROM telegram.telegram_users WHERE telegram_id = 100001),
                'thumbs_up',
                '2026-02-01T10:07:00Z',
                NOW()
            ),
            (
                (
                    SELECT id
                    FROM telegram.telegram_messages
                    WHERE channel_id = (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000001)
                      AND telegram_message_id = 11
                ),
                (SELECT id FROM telegram.telegram_users WHERE telegram_id = 100002),
                'fire',
                '2026-02-01T10:08:00Z',
                NOW()
            )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO telegram.telegram_service_actions
            (message_id, action, actor_id, metadata, created_at)
        VALUES
            (
                (
                    SELECT id
                    FROM telegram.telegram_messages
                    WHERE channel_id = (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000002)
                      AND telegram_message_id = 20
                ),
                'message_pinned',
                (SELECT id FROM telegram.telegram_users WHERE telegram_id = 100002),
                '{"source":"fixture"}'::jsonb,
                '2026-02-01T11:01:00Z'
            )
        "#,
    )
    .await?;

    Ok(())
}

async fn execute_sql(db: &impl ConnectionTrait, sql: &str) -> anyhow::Result<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        vec![],
    ))
    .await?;

    Ok(())
}
