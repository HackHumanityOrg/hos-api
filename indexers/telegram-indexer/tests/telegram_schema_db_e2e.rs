mod support;

use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use support::{init_test_tracing, TestDatabase};

#[tokio::test]
async fn telegram_schema_tables_exist_after_migrations() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();

    let table_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM information_schema.tables
         WHERE table_schema = 'telegram'
            AND table_name IN (
             'telegram_channels',
             'telegram_users',
             'telegram_messages',
             'telegram_reactions',
             'telegram_service_actions'
            )",
    )
    .await?;

    assert_eq!(table_count, 5);
    Ok(())
}

#[tokio::test]
async fn telegram_schema_enforces_unique_and_fk_behaviors() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();

    execute_sql(
        conn.as_ref(),
        "INSERT INTO telegram.telegram_channels
            (telegram_id, name, username, type, created_at, updated_at)
         VALUES
            (-1005000000001, 'House of Stake', 'houseofstake', 'channel', NOW(), NOW())",
    )
    .await?;

    let duplicate_channel = execute_sql(
        conn.as_ref(),
        "INSERT INTO telegram.telegram_channels
            (telegram_id, name, username, type, created_at, updated_at)
         VALUES
            (-1005000000001, 'Duplicate', 'duplicate', 'channel', NOW(), NOW())",
    )
    .await;

    assert!(duplicate_channel.is_err(), "expected unique key violation");

    execute_sql(
        conn.as_ref(),
        "INSERT INTO telegram.telegram_users
            (telegram_id, username, display_name, created_at, updated_at)
         VALUES
            (100001, 'alice', 'Alice', NOW(), NOW())",
    )
    .await?;

    execute_sql(
        conn.as_ref(),
        "INSERT INTO telegram.telegram_messages
            (channel_id, telegram_message_id, message_type, from_user_id, reply_to_message_id,
             reply_to_telegram_message_id, date, edited_date, text, text_entities, has_media,
             media_type, media_path, created_at, updated_at)
         VALUES
            (
              (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000001),
              42,
              'message',
              (SELECT id FROM telegram.telegram_users WHERE telegram_id = 100001),
              NULL,
              NULL,
              NOW(),
              NULL,
              'Hello',
              NULL,
              FALSE,
              NULL,
              NULL,
              NOW(),
              NOW()
            )",
    )
    .await?;

    execute_sql(
        conn.as_ref(),
        "INSERT INTO telegram.telegram_reactions
            (message_id, user_id, emoji, date, created_at)
         VALUES
            (
              (
                SELECT id FROM telegram.telegram_messages
                WHERE channel_id = (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000001)
                  AND telegram_message_id = 42
              ),
              (SELECT id FROM telegram.telegram_users WHERE telegram_id = 100001),
              '👍',
              NOW(),
              NOW()
            )",
    )
    .await?;

    execute_sql(
        conn.as_ref(),
        "DELETE FROM telegram.telegram_messages
         WHERE channel_id = (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000001)
           AND telegram_message_id = 42",
    )
    .await?;

    let reaction_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM telegram.telegram_reactions",
    )
    .await?;
    assert_eq!(reaction_count, 0, "expected reaction cascade delete");

    execute_sql(
        conn.as_ref(),
        "INSERT INTO telegram.telegram_messages
            (channel_id, telegram_message_id, message_type, from_user_id, reply_to_message_id,
             reply_to_telegram_message_id, date, edited_date, text, text_entities, has_media,
             media_type, media_path, created_at, updated_at)
         VALUES
            (
              (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000001),
              43,
              'service',
              (SELECT id FROM telegram.telegram_users WHERE telegram_id = 100001),
              NULL,
              NULL,
              NOW(),
              NULL,
              NULL,
              NULL,
              FALSE,
              NULL,
              NULL,
              NOW(),
              NOW()
            )",
    )
    .await?;

    execute_sql(
        conn.as_ref(),
        "INSERT INTO telegram.telegram_service_actions
            (message_id, action, actor_id, metadata, created_at)
         VALUES
            (
              (
                SELECT id FROM telegram.telegram_messages
                WHERE channel_id = (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000001)
                  AND telegram_message_id = 43
              ),
              'message_pinned',
              (SELECT id FROM telegram.telegram_users WHERE telegram_id = 100001),
              NULL,
              NOW()
            )",
    )
    .await?;

    execute_sql(
        conn.as_ref(),
        "DELETE FROM telegram.telegram_users WHERE telegram_id = 100001",
    )
    .await?;

    let from_user_is_null = scalar_i64(
        conn.as_ref(),
        "SELECT CASE WHEN from_user_id IS NULL THEN 1::bigint ELSE 0::bigint END AS value
         FROM telegram.telegram_messages
         WHERE channel_id = (SELECT id FROM telegram.telegram_channels WHERE telegram_id = -1005000000001)
           AND telegram_message_id = 43",
    )
    .await?;
    assert_eq!(
        from_user_is_null, 1,
        "expected message.from_user_id to be set null"
    );

    let actor_is_null = scalar_i64(
        conn.as_ref(),
        "SELECT CASE WHEN actor_id IS NULL THEN 1::bigint ELSE 0::bigint END AS value
         FROM telegram.telegram_service_actions
         WHERE action = 'message_pinned'",
    )
    .await?;
    assert_eq!(
        actor_is_null, 1,
        "expected service action actor_id to be set null"
    );

    Ok(())
}

async fn scalar_i64(db: &impl ConnectionTrait, sql: &str) -> Result<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![],
        ))
        .await?
        .context("expected one row")?;

    Ok(row.try_get("", "value")?)
}

async fn execute_sql(db: &impl ConnectionTrait, sql: &str) -> Result<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        vec![],
    ))
    .await?;

    Ok(())
}
