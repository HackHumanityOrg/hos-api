use anyhow::Result;
use discourse_indexer::discourse_api::DiscourseClient;
use discourse_indexer::domain::instances;
use discourse_indexer::sync::{self, Concurrency};
use entities::{
    discourse::{
        discourse_categories, discourse_post_likes, discourse_post_revisions, discourse_posts,
        discourse_topics, discourse_users,
    },
    prelude::{
        DiscourseCategories, DiscoursePostLikes, DiscoursePostRevisions, DiscoursePosts,
        DiscourseTopics, DiscourseUsers,
    },
};
use mockito::Server;
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter};
use serde_json::json;
use std::time::Duration;
use test_support::{init_test_tracing, TestDatabase};

const INSTANCE_KEY: &str = "sync-mock";

#[tokio::test]
async fn sync_recent_runs_end_to_end_against_mock_upstream_and_postgres() -> Result<()> {
    init_test_tracing();

    let mut server = Server::new_async().await;
    let category_page_0 = server
        .mock("GET", "/categories.json?include_subcategories=true&page=0")
        .with_status(200)
        .with_body(categories_payload(vec![minimal_category(10)]).to_string())
        .expect(1)
        .create_async()
        .await;
    let category_page_1 = server
        .mock("GET", "/categories.json?include_subcategories=true&page=1")
        .with_status(200)
        .with_body(categories_payload(vec![]).to_string())
        .expect(1)
        .create_async()
        .await;

    let topics_page_0 = server
        .mock("GET", "/latest.json?page=0")
        .with_status(200)
        .with_body(topics_payload(vec![minimal_topic(501, 10)]).to_string())
        .expect(1)
        .create_async()
        .await;

    let posts_page_0 = server
        .mock("GET", "/t/501.json?page=1&include_raw=true")
        .with_status(200)
        .with_body(posts_payload(vec![minimal_post(9001, 501, "alice", 100)], 1).to_string())
        .expect(1)
        .create_async()
        .await;

    let user_profile = server
        .mock("GET", "/u/alice.json")
        .with_status(200)
        .with_body(user_profile_payload(100, "alice").to_string())
        .expect(1)
        .create_async()
        .await;

    let db = TestDatabase::new().await;
    let conn = db.db();

    instances::repo::register_instance(conn.as_ref(), INSTANCE_KEY, &server.url()).await?;

    let client = DiscourseClient::new(server.url());
    let concurrency = Concurrency::new(1, 1, 1);

    tokio::time::timeout(
        Duration::from_secs(30),
        sync::sync_recent(conn.clone(), &client, INSTANCE_KEY, 1, &concurrency, 5, 24),
    )
    .await
    .expect("sync_recent timeout")?;

    assert_eq!(count_categories(conn.as_ref(), INSTANCE_KEY).await?, 1);
    assert_eq!(count_topics(conn.as_ref(), INSTANCE_KEY).await?, 1);
    assert_eq!(count_posts(conn.as_ref(), INSTANCE_KEY).await?, 1);
    assert_eq!(count_users(conn.as_ref(), INSTANCE_KEY).await?, 1);
    assert_eq!(count_likes(conn.as_ref(), INSTANCE_KEY).await?, 0);
    assert_eq!(count_revisions(conn.as_ref(), INSTANCE_KEY).await?, 0);

    category_page_0.assert_async().await;
    category_page_1.assert_async().await;
    topics_page_0.assert_async().await;
    posts_page_0.assert_async().await;
    user_profile.assert_async().await;

    Ok(())
}

#[tokio::test]
async fn sync_full_includes_directory_users_flow() -> Result<()> {
    init_test_tracing();

    let mut server = Server::new_async().await;
    let category_page_0 = server
        .mock("GET", "/categories.json?include_subcategories=true&page=0")
        .with_status(200)
        .with_body(categories_payload(vec![minimal_category(10)]).to_string())
        .expect(1)
        .create_async()
        .await;
    let category_page_1 = server
        .mock("GET", "/categories.json?include_subcategories=true&page=1")
        .with_status(200)
        .with_body(categories_payload(vec![]).to_string())
        .expect(1)
        .create_async()
        .await;

    let topics_page_0 = server
        .mock("GET", "/latest.json?order=created&ascending=true&page=0")
        .with_status(200)
        .with_body(topics_payload(vec![minimal_topic(502, 10)]).to_string())
        .expect(1)
        .create_async()
        .await;
    let topics_page_1 = server
        .mock("GET", "/latest.json?order=created&ascending=true&page=1")
        .with_status(200)
        .with_body(topics_payload(vec![]).to_string())
        .expect(1)
        .create_async()
        .await;

    let posts_page_0 = server
        .mock("GET", "/t/502.json?page=1&include_raw=true")
        .with_status(200)
        .with_body(posts_payload(vec![minimal_post(9002, 502, "alice", 100)], 1).to_string())
        .expect(1)
        .create_async()
        .await;

    let user_profile = server
        .mock("GET", "/u/alice.json")
        .with_status(200)
        .with_body(user_profile_payload(100, "alice").to_string())
        .expect(1)
        .create_async()
        .await;

    let directory_page_0 = server
        .mock(
            "GET",
            "/directory_items.json?period=all&order=username&page=0&asc=true",
        )
        .with_status(200)
        .with_body(directory_payload(vec![directory_item(200, "directory_user")]).to_string())
        .expect(1)
        .create_async()
        .await;
    let directory_page_1 = server
        .mock(
            "GET",
            "/directory_items.json?period=all&order=username&page=1&asc=true",
        )
        .with_status(200)
        .with_body(directory_payload(vec![]).to_string())
        .expect(1)
        .create_async()
        .await;

    let db = TestDatabase::new().await;
    let conn = db.db();

    instances::repo::register_instance(conn.as_ref(), INSTANCE_KEY, &server.url()).await?;

    let client = DiscourseClient::new(server.url());
    let concurrency = Concurrency::new(1, 1, 1);

    tokio::time::timeout(
        Duration::from_secs(30),
        sync::sync_full(conn.clone(), &client, INSTANCE_KEY, &concurrency, 5),
    )
    .await
    .expect("sync_full timeout")?;

    assert_eq!(count_categories(conn.as_ref(), INSTANCE_KEY).await?, 1);
    assert_eq!(count_topics(conn.as_ref(), INSTANCE_KEY).await?, 1);
    assert_eq!(count_posts(conn.as_ref(), INSTANCE_KEY).await?, 1);
    assert_eq!(count_users(conn.as_ref(), INSTANCE_KEY).await?, 2);
    assert_eq!(count_likes(conn.as_ref(), INSTANCE_KEY).await?, 0);
    assert_eq!(count_revisions(conn.as_ref(), INSTANCE_KEY).await?, 0);

    category_page_0.assert_async().await;
    category_page_1.assert_async().await;
    topics_page_0.assert_async().await;
    topics_page_1.assert_async().await;
    posts_page_0.assert_async().await;
    user_profile.assert_async().await;
    directory_page_0.assert_async().await;
    directory_page_1.assert_async().await;

    Ok(())
}

#[tokio::test]
async fn sync_recent_fetches_likes_and_revisions_when_available() -> Result<()> {
    init_test_tracing();

    let mut server = Server::new_async().await;
    let category_page_0 = server
        .mock("GET", "/categories.json?include_subcategories=true&page=0")
        .with_status(200)
        .with_body(categories_payload(vec![minimal_category(10)]).to_string())
        .expect(1)
        .create_async()
        .await;
    let category_page_1 = server
        .mock("GET", "/categories.json?include_subcategories=true&page=1")
        .with_status(200)
        .with_body(categories_payload(vec![]).to_string())
        .expect(1)
        .create_async()
        .await;

    let topics_page_0 = server
        .mock("GET", "/latest.json?page=0")
        .with_status(200)
        .with_body(topics_payload(vec![minimal_topic(503, 10)]).to_string())
        .expect(1)
        .create_async()
        .await;

    let posts_page_0 = server
        .mock("GET", "/t/503.json?page=1&include_raw=true")
        .with_status(200)
        .with_body(
            posts_payload(
                vec![json!({
                    "id": 9003,
                    "name": "alice",
                    "username": "alice",
                    "avatar_template": "/user_avatar/example/{size}/1_2.png",
                    "created_at": "2099-01-01T00:00:00Z",
                    "cooked": null,
                    "raw": "post with likes and edits",
                    "post_number": 1,
                    "post_type": 1,
                    "updated_at": "2099-01-01T00:00:00Z",
                    "reply_count": 0,
                    "reply_to_post_number": null,
                    "quote_count": 0,
                    "incoming_link_count": 0,
                    "reads": 1,
                    "readers_count": 1,
                    "score": 1.0,
                    "topic_id": 503,
                    "topic_slug": "topic-503",
                    "display_username": "alice",
                    "primary_group_name": null,
                    "flair_name": null,
                    "flair_url": null,
                    "flair_bg_color": null,
                    "flair_color": null,
                    "version": 2,
                    "can_view_edit_history": true,
                    "user_id": 100,
                    "actions_summary": [{"id": 2, "count": 1}],
                    "deleted": false
                })],
                1,
            )
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let alice_profile = server
        .mock("GET", "/u/alice.json")
        .with_status(200)
        .with_body(user_profile_payload(100, "alice").to_string())
        .expect(1)
        .create_async()
        .await;
    let bob_profile = server
        .mock("GET", "/u/bob.json")
        .with_status(200)
        .with_body(user_profile_payload(200, "bob").to_string())
        .expect(1)
        .create_async()
        .await;

    let likes_page = server
        .mock(
            "GET",
            "/post_action_users.json?id=9003&post_action_type_id=2&page=0&limit=50",
        )
        .with_status(200)
        .with_body(likes_payload().to_string())
        .expect(1)
        .create_async()
        .await;

    let revision = server
        .mock("GET", "/posts/9003/revisions/2.json")
        .with_status(200)
        .with_body(revision_payload(9003, 2, "alice").to_string())
        .expect(1)
        .create_async()
        .await;

    let db = TestDatabase::new().await;
    let conn = db.db();
    instances::repo::register_instance(conn.as_ref(), INSTANCE_KEY, &server.url()).await?;

    let client = DiscourseClient::new(server.url());
    let concurrency = Concurrency::new(2, 2, 2);

    tokio::time::timeout(
        Duration::from_secs(30),
        sync::sync_recent(conn.clone(), &client, INSTANCE_KEY, 1, &concurrency, 5, 24),
    )
    .await
    .expect("sync_recent timeout")?;

    assert_eq!(count_topics(conn.as_ref(), INSTANCE_KEY).await?, 1);
    assert_eq!(count_posts(conn.as_ref(), INSTANCE_KEY).await?, 1);
    assert_eq!(count_likes(conn.as_ref(), INSTANCE_KEY).await?, 1);
    assert_eq!(count_revisions(conn.as_ref(), INSTANCE_KEY).await?, 1);
    assert_eq!(count_users(conn.as_ref(), INSTANCE_KEY).await?, 2);

    category_page_0.assert_async().await;
    category_page_1.assert_async().await;
    topics_page_0.assert_async().await;
    posts_page_0.assert_async().await;
    alice_profile.assert_async().await;
    bob_profile.assert_async().await;
    likes_page.assert_async().await;
    revision.assert_async().await;

    Ok(())
}

fn categories_payload(categories: Vec<serde_json::Value>) -> serde_json::Value {
    json!({
        "category_list": {
            "categories": categories,
        }
    })
}

fn minimal_category(id: i32) -> serde_json::Value {
    json!({
        "id": id,
        "name": format!("Category {id}"),
        "color": "0088CC",
        "text_color": "FFFFFF",
        "slug": format!("category-{id}"),
        "topic_count": 1,
        "post_count": 1,
        "position": 0,
        "description": null,
        "description_text": null,
        "description_excerpt": null,
        "topic_url": null,
        "read_restricted": false,
        "notification_level": null,
        "topic_template": null,
        "has_children": false,
        "subcategory_count": 0,
        "sort_order": null,
        "sort_ascending": null,
        "show_subcategory_list": false,
        "num_featured_topics": 0,
        "default_view": null,
        "subcategory_list_style": "",
        "default_top_period": "all",
        "default_list_filter": "all",
        "minimum_required_tags": 0,
        "navigate_to_first_post_after_read": false,
        "topics_day": 0,
        "topics_week": 0,
        "topics_month": 0,
        "topics_year": 0,
        "topics_all_time": 1,
        "subcategory_list": []
    })
}

fn topics_payload(topics: Vec<serde_json::Value>) -> serde_json::Value {
    json!({
        "topic_list": {
            "topics": topics,
            "per_page": 30
        }
    })
}

fn minimal_topic(id: i32, category_id: i32) -> serde_json::Value {
    json!({
        "id": id,
        "title": format!("Topic {id}"),
        "fancy_title": format!("Topic {id}"),
        "slug": format!("topic-{id}"),
        "posts_count": 1,
        "reply_count": 0,
        "highest_post_number": 1,
        "image_url": null,
        "created_at": "2099-01-01T00:00:00Z",
        "last_posted_at": "2099-01-01T00:00:00Z",
        "bumped": true,
        "bumped_at": "2099-01-01T00:00:00Z",
        "archetype": "regular",
        "pinned": false,
        "visible": true,
        "closed": false,
        "archived": false,
        "views": 5,
        "like_count": 0,
        "has_summary": false,
        "last_poster_username": "alice",
        "category_id": category_id,
        "pinned_globally": false
    })
}

fn posts_payload(posts: Vec<serde_json::Value>, posts_count: i32) -> serde_json::Value {
    json!({
        "post_stream": {
            "posts": posts
        },
        "posts_count": posts_count
    })
}

fn minimal_post(id: i32, topic_id: i32, username: &str, user_id: i32) -> serde_json::Value {
    json!({
        "id": id,
        "name": username,
        "username": username,
        "avatar_template": "/user_avatar/example/{size}/1_2.png",
        "created_at": "2026-01-01T00:00:00Z",
        "cooked": null,
        "raw": "hello world",
        "post_number": 1,
        "post_type": 1,
        "updated_at": "2026-01-01T00:00:00Z",
        "reply_count": 0,
        "reply_to_post_number": null,
        "quote_count": 0,
        "incoming_link_count": 0,
        "reads": 1,
        "readers_count": 1,
        "score": 0.0,
        "topic_id": topic_id,
        "topic_slug": format!("topic-{topic_id}"),
        "display_username": username,
        "primary_group_name": null,
        "flair_name": null,
        "flair_url": null,
        "flair_bg_color": null,
        "flair_color": null,
        "version": 1,
        "can_view_edit_history": false,
        "user_id": user_id,
        "actions_summary": [],
        "deleted": false
    })
}

fn user_profile_payload(id: i32, username: &str) -> serde_json::Value {
    json!({
        "user": {
            "id": id,
            "username": username,
            "name": username,
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
}

fn directory_payload(items: Vec<serde_json::Value>) -> serde_json::Value {
    json!({
        "directory_items": items,
        "meta": {
            "last_updated_at": "2026-01-01T00:00:00.000Z",
            "total_rows_directory_items": 1,
            "load_more_directory_items": null
        }
    })
}

fn directory_item(id: i32, username: &str) -> serde_json::Value {
    json!({
        "id": id,
        "likes_received": 0,
        "likes_given": 0,
        "topics_entered": 0,
        "topic_count": 0,
        "post_count": 0,
        "posts_read": 0,
        "days_visited": 0,
        "user": {
            "id": id,
            "username": username,
            "name": username,
            "avatar_template": "/user_avatar/example/{size}/1_2.png",
            "title": null,
            "trust_level": 1
        }
    })
}

fn likes_payload() -> serde_json::Value {
    json!({
        "post_action_users": [
            {
                "id": 200,
                "username": "bob",
                "name": "bob",
                "avatar_template": "/user_avatar/example/{size}/1_2.png",
                "post_url": null,
                "username_lower": "bob"
            }
        ],
        "total_rows_post_action_users": 1
    })
}

fn revision_payload(post_id: i32, version: i32, username: &str) -> serde_json::Value {
    json!({
        "created_at": "2099-01-01T00:00:00Z",
        "post_id": post_id,
        "previous_hidden": false,
        "current_hidden": false,
        "first_revision": 1,
        "previous_revision": 1,
        "current_revision": version,
        "next_revision": null,
        "last_revision": version,
        "current_version": version,
        "version_count": version,
        "username": username,
        "display_username": username,
        "avatar_template": "/user_avatar/example/{size}/1_2.png",
        "edit_reason": null,
        "body_changes": {
            "inline": "",
            "side_by_side": "",
            "side_by_side_markdown": "<table><tr><td><del>before</del></td><td><ins>after</ins></td></tr></table>"
        },
        "title_changes": {
            "inline": "",
            "side_by_side": "<div class=\"revision-content\"><div>Old</div><div>New</div></div>"
        }
    })
}

async fn count_categories(db: &sea_orm::DatabaseConnection, instance: &str) -> Result<u64> {
    Ok(DiscourseCategories::find()
        .filter(discourse_categories::Column::Instance.eq(instance))
        .count(db)
        .await?)
}

async fn count_topics(db: &sea_orm::DatabaseConnection, instance: &str) -> Result<u64> {
    Ok(DiscourseTopics::find()
        .filter(discourse_topics::Column::Instance.eq(instance))
        .count(db)
        .await?)
}

async fn count_posts(db: &sea_orm::DatabaseConnection, instance: &str) -> Result<u64> {
    Ok(DiscoursePosts::find()
        .filter(discourse_posts::Column::Instance.eq(instance))
        .count(db)
        .await?)
}

async fn count_users(db: &sea_orm::DatabaseConnection, instance: &str) -> Result<u64> {
    Ok(DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(instance))
        .count(db)
        .await?)
}

async fn count_likes(db: &sea_orm::DatabaseConnection, instance: &str) -> Result<u64> {
    Ok(DiscoursePostLikes::find()
        .filter(discourse_post_likes::Column::Instance.eq(instance))
        .count(db)
        .await?)
}

async fn count_revisions(db: &sea_orm::DatabaseConnection, instance: &str) -> Result<u64> {
    Ok(DiscoursePostRevisions::find()
        .filter(discourse_post_revisions::Column::Instance.eq(instance))
        .count(db)
        .await?)
}
