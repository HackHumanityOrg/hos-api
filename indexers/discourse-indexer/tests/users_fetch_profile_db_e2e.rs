use anyhow::Result;
use discourse_indexer::discourse_api::DiscourseClient;
use discourse_indexer::domain::instances;
use discourse_indexer::domain::users::{adapter, model::DirectoryItem, model::User, repo};
use entities::{discourse::discourse_users, prelude::DiscourseUsers};
use mockito::Server;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use test_support::{init_test_tracing, TestDatabase};

const INSTANCE_KEY: &str = "users-fetch";

#[tokio::test]
async fn fetch_user_by_username_creates_placeholder_when_upstream_is_missing() -> Result<()> {
    init_test_tracing();

    let mut server = Server::new_async().await;
    let not_found = server
        .mock("GET", "/u/missing-user.json")
        .with_status(404)
        .expect(1)
        .create_async()
        .await;

    let db = TestDatabase::new().await;
    let conn = db.db();
    instances::repo::register_instance(conn.as_ref(), INSTANCE_KEY, &server.url()).await?;

    let client = DiscourseClient::new(server.url());
    let known_users = adapter::KnownUsersCache::new();
    adapter::fetch_user_by_username(
        conn.as_ref(),
        &client,
        INSTANCE_KEY,
        "missing-user",
        424242,
        &known_users,
    )
    .await?;

    let row = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(INSTANCE_KEY))
        .filter(discourse_users::Column::ExternalId.eq(424242))
        .one(conn.as_ref())
        .await?
        .expect("placeholder user should be stored");

    assert_eq!(row.username, "missing-user");
    assert_eq!(row.name.as_deref(), Some("[Deleted User: missing-user]"));
    assert_eq!(row.title.as_deref(), Some("[Deleted]"));

    not_found.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn fetch_user_by_username_skips_remote_call_when_user_is_already_present() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();
    instances::repo::register_instance(conn.as_ref(), INSTANCE_KEY, "https://example.invalid")
        .await?;

    let seeded = DirectoryItem {
        id: 100,
        likes_received: Some(0),
        likes_given: Some(0),
        topics_entered: Some(0),
        topic_count: Some(0),
        post_count: Some(0),
        posts_read: Some(0),
        days_visited: Some(0),
        user: User {
            id: 100,
            username: "already-present".to_string(),
            name: Some("Already Present".to_string()),
            avatar_template: "/user_avatar/example/{size}/1_2.png".to_string(),
            title: None,
            trust_level: 1,
            likes_received: None,
            likes_given: None,
            topics_entered: None,
            topic_count: None,
            post_count: None,
            posts_read: None,
            days_visited: None,
        },
    };
    repo::upsert_user(conn.as_ref(), INSTANCE_KEY, &seeded).await?;

    let client = DiscourseClient::new("http://127.0.0.1:9".to_string());
    let known_users = adapter::KnownUsersCache::new();
    adapter::fetch_user_by_username(
        conn.as_ref(),
        &client,
        INSTANCE_KEY,
        "already-present",
        100,
        &known_users,
    )
    .await?;

    let row = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(INSTANCE_KEY))
        .filter(discourse_users::Column::ExternalId.eq(100))
        .one(conn.as_ref())
        .await?
        .expect("seeded user should remain present");
    assert_eq!(row.username, "already-present");

    Ok(())
}

#[tokio::test]
async fn fetch_user_by_username_deduplicates_within_same_run() -> Result<()> {
    init_test_tracing();

    let mut server = Server::new_async().await;
    let profile = server
        .mock("GET", "/u/race-user.json")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "user": {
                    "id": 777,
                    "username": "race-user",
                    "name": "Race User",
                    "avatar_template": "/user_avatar/example/{size}/1_2.png",
                    "title": null,
                    "trust_level": 1,
                    "bio_raw": null,
                    "bio_cooked": null,
                    "bio_excerpt": null,
                    "profile_view_count": 0,
                    "location": null,
                    "website": null,
                    "website_name": null,
                    "can_edit": false,
                    "can_edit_username": false,
                    "can_edit_email": false,
                    "can_edit_name": false,
                    "flair_name": null,
                    "flair_url": null,
                    "flair_bg_color": null,
                    "flair_color": null,
                    "primary_group_name": null,
                    "primary_group_id": null,
                    "moderator": false,
                    "admin": false,
                    "last_posted_at": null,
                    "last_seen_at": null,
                    "created_at": "2026-01-01T00:00:00Z",
                    "time_read": 0,
                    "badge_count": 0
                }
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let db = TestDatabase::new().await;
    let conn = db.db();
    instances::repo::register_instance(conn.as_ref(), INSTANCE_KEY, &server.url()).await?;

    let client = DiscourseClient::new(server.url());
    let known_users = adapter::KnownUsersCache::new();

    let (first, second) = tokio::join!(
        adapter::fetch_user_by_username(
            conn.as_ref(),
            &client,
            INSTANCE_KEY,
            "race-user",
            777,
            &known_users,
        ),
        adapter::fetch_user_by_username(
            conn.as_ref(),
            &client,
            INSTANCE_KEY,
            "race-user",
            777,
            &known_users,
        )
    );

    first?;
    second?;

    let row = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(INSTANCE_KEY))
        .filter(discourse_users::Column::ExternalId.eq(777))
        .one(conn.as_ref())
        .await?
        .expect("race user should be stored once");
    assert_eq!(row.username, "race-user");

    profile.assert_async().await;
    Ok(())
}

#[tokio::test]
async fn known_users_cache_is_case_insensitive() {
    let cache = adapter::KnownUsersCache::new();

    assert!(cache.try_mark_seen("Alice").await);
    assert!(!cache.try_mark_seen("alice").await);
}
