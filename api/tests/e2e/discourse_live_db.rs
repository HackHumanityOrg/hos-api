use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use tower::ServiceExt;

#[path = "../support/mod.rs"]
mod support;

#[tokio::test]
async fn discourse_endpoints_use_canonical_identifier_fields() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;
    seed_discourse_data(database.db().as_ref())
        .await
        .expect("seed discourse data");

    let instances = get_json_value(&router, "/api/v1/discourse/instances").await;
    let instance = instances
        .get("data")
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .and_then(|value| value.as_object())
        .expect("instances data row");
    assert_eq!(
        instance.get("instance").and_then(|value| value.as_str()),
        Some("NEAR")
    );
    assert!(instance.get("key").is_none());

    let instance_detail = get_json_value(&router, "/api/v1/discourse/instances/NEAR").await;
    let instance_detail = instance_detail.as_object().expect("instance detail object");
    assert_eq!(
        instance_detail
            .get("instance")
            .and_then(|value| value.as_str()),
        Some("NEAR")
    );
    assert!(instance_detail.get("key").is_none());

    let category = get_json_value(&router, "/api/v1/discourse/instances/NEAR/categories/7").await;
    let category = category.as_object().expect("category object");
    assert_eq!(category.get("category_id").and_then(|value| value.as_i64()), Some(7));
    assert!(category.get("external_id").is_none());

    let topic = get_json_value(&router, "/api/v1/discourse/instances/NEAR/topics/9").await;
    let topic = topic.as_object().expect("topic object");
    assert_eq!(topic.get("topic_id").and_then(|value| value.as_i64()), Some(9));
    assert!(topic.get("external_id").is_none());

    let post = get_json_value(&router, "/api/v1/discourse/instances/NEAR/posts/15").await;
    let post = post.as_object().expect("post object");
    assert_eq!(post.get("post_id").and_then(|value| value.as_i64()), Some(15));
    assert!(post.get("external_id").is_none());

    let likes = get_json_value(&router, "/api/v1/discourse/instances/NEAR/posts/15/likes").await;
    let like = likes
        .get("data")
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .and_then(|value| value.as_object())
        .expect("likes data row");
    assert_eq!(like.get("post_id").and_then(|value| value.as_i64()), Some(15));
    assert_eq!(like.get("user_id").and_then(|value| value.as_i64()), Some(42));
    assert!(like.get("external_post_id").is_none());
    assert!(like.get("external_user_id").is_none());

    let revisions =
        get_json_value(&router, "/api/v1/discourse/instances/NEAR/posts/15/revisions").await;
    let revision = revisions
        .get("data")
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .and_then(|value| value.as_object())
        .expect("revisions data row");
    assert_eq!(
        revision.get("post_id").and_then(|value| value.as_i64()),
        Some(15)
    );
    assert!(revision.get("external_post_id").is_none());

    let user = get_json_value(&router, "/api/v1/discourse/instances/NEAR/users/42").await;
    let user = user.as_object().expect("user object");
    assert_eq!(user.get("user_id").and_then(|value| value.as_i64()), Some(42));
    assert!(user.get("external_id").is_none());
}

async fn get_json_value(router: &Router, path: &str) -> serde_json::Value {
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

async fn seed_discourse_data(db: &impl ConnectionTrait) -> anyhow::Result<()> {
    execute_sql(
        db,
        r#"
        INSERT INTO discourse.discourse_instances (key, base_url, enabled, created_at, updated_at)
        VALUES ('NEAR', 'https://gov.near.org', TRUE, NOW(), NOW())
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO discourse.discourse_categories (
            id,
            instance,
            external_id,
            name,
            color,
            text_color,
            slug,
            topic_count,
            post_count,
            description,
            description_text,
            topics_day,
            topics_week,
            topics_month,
            topics_year,
            topics_all_time,
            parent_category_id,
            position,
            read_restricted
        )
        VALUES (
            1,
            'NEAR',
            7,
            'Governance',
            '00AAFF',
            'FFFFFF',
            'governance',
            1,
            1,
            NULL,
            NULL,
            NULL,
            NULL,
            NULL,
            NULL,
            NULL,
            NULL,
            1,
            FALSE
        )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO discourse.discourse_users (
            id,
            instance,
            external_id,
            username,
            name,
            avatar_template,
            title,
            likes_received,
            likes_given,
            topics_entered,
            topic_count,
            post_count,
            posts_read,
            days_visited,
            trust_level
        )
        VALUES (
            1,
            'NEAR',
            42,
            'alice',
            'Alice',
            '/user_avatar/gov.near.org/alice/{size}/1_2.png',
            NULL,
            10,
            5,
            7,
            1,
            1,
            100,
            30,
            2
        )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO discourse.discourse_topics (
            id,
            instance,
            external_id,
            title,
            fancy_title,
            slug,
            posts_count,
            reply_count,
            created_at,
            last_posted_at,
            bumped_at,
            pinned,
            pinned_globally,
            visible,
            closed,
            archived,
            views,
            like_count,
            external_category_id,
            highest_post_number,
            archetype,
            image_url
        )
        VALUES (
            10,
            'NEAR',
            9,
            'Governance update',
            'Governance update',
            'governance-update',
            1,
            0,
            '2026-01-01T00:00:00Z',
            '2026-01-01T00:00:00Z',
            '2026-01-01T00:00:00Z',
            FALSE,
            FALSE,
            TRUE,
            FALSE,
            FALSE,
            10,
            2,
            7,
            1,
            'regular',
            NULL
        )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO discourse.discourse_posts (
            id,
            instance,
            external_id,
            name,
            username,
            created_at,
            post_number,
            post_type,
            updated_at,
            reply_count,
            reply_to_post_number,
            quote_count,
            incoming_link_count,
            reads,
            readers_count,
            score,
            external_topic_id,
            topic_slug,
            display_username,
            primary_group_name,
            flair_name,
            flair_url,
            flair_bg_color,
            flair_color,
            version,
            external_user_id,
            can_view_edit_history,
            deleted,
            raw
        )
        VALUES (
            100,
            'NEAR',
            15,
            'Alice',
            'alice',
            '2026-01-01T00:05:00Z',
            1,
            1,
            '2026-01-01T00:06:00Z',
            0,
            NULL,
            0,
            0,
            10,
            8,
            1.5,
            9,
            'governance-update',
            NULL,
            NULL,
            NULL,
            NULL,
            NULL,
            NULL,
            1,
            42,
            TRUE,
            FALSE,
            'hello world'
        )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO discourse.discourse_post_likes (id, instance, external_post_id, external_user_id, created_at)
        VALUES (1000, 'NEAR', 15, 42, '2026-01-01T00:07:00Z')
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO discourse.discourse_post_revisions (
            id,
            instance,
            external_post_id,
            version,
            created_at,
            username,
            edit_reason,
            raw_body_before,
            raw_body_after,
            raw_title_before,
            raw_title_after,
            body_changes,
            title_changes
        )
        VALUES (
            2000,
            'NEAR',
            15,
            1,
            '2026-01-01T00:08:00Z',
            'alice',
            'typo',
            'hello',
            'hello world',
            NULL,
            NULL,
            NULL,
            NULL
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
