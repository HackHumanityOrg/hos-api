#[path = "common/mod.rs"]
mod common;

use common::TestDatabase;
use discourse_indexer::discourse_api::DiscourseClient;
use discourse_indexer::sync::{self, Concurrency};
use entities::{discourse_posts, discourse_topics};
use mockito::Server;
use rstest::*;
use sea_orm::{EntityTrait, PaginatorTrait};

#[fixture]
async fn test_db() -> TestDatabase {
    TestDatabase::new().await
}

#[rstest]
#[tokio::test]
async fn test_sync_recent_with_mock_server_inserts_topic_and_post(#[future] test_db: TestDatabase) {
    let test_db = test_db.await;
    let db = test_db.db();
    let mut server = Server::new_async().await;
    let base = server.url();

    // Preseed instance and a minimal category for FK constraints
    discourse_indexer::domain::instances::repo::register_instance(&db, "mock.instance", &base)
        .await
        .expect("register instance for mock");

    let seed_cat = discourse_indexer::domain::categories::model::Category {
        id: 1,
        name: "General".to_string(),
        color: "FFFFFF".to_string(),
        text_color: "000000".to_string(),
        slug: "general".to_string(),
        topic_count: 0,
        post_count: 0,
        position: 0,
        description: None,
        description_text: None,
        description_excerpt: None,
        topic_url: None,
        read_restricted: false,
        permission: None,
        notification_level: None,
        can_edit: Some(false),
        topic_template: None,
        has_children: false,
        subcategory_count: Some(0),
        sort_order: None,
        sort_ascending: None,
        show_subcategory_list: false,
        num_featured_topics: Some(0),
        default_view: None,
        subcategory_list_style: "rows_with_featured_topics".to_string(),
        default_top_period: "all".to_string(),
        default_list_filter: "all".to_string(),
        minimum_required_tags: Some(0),
        navigate_to_first_post_after_read: false,
        topics_day: 0,
        topics_week: 0,
        topics_month: 0,
        topics_year: 0,
        topics_all_time: 0,
        uploaded_logo: None,
        uploaded_logo_dark: None,
        uploaded_background: None,
        uploaded_background_dark: None,
        subcategory_list: None,
        is_uncategorized: Some(false),
    };
    discourse_indexer::domain::categories::repo::upsert_category(&db, "mock.instance", &seed_cat)
        .await
        .expect("seed category for FK");

    // Preseed minimal author user referenced by the mocked post (user_id = 1)
    let seed_user = discourse_indexer::domain::users::model::DirectoryItem {
        id: 0,
        likes_received: 0,
        likes_given: 0,
        topics_entered: 0,
        topic_count: 0,
        post_count: 0,
        posts_read: 0,
        days_visited: 0,
        user: discourse_indexer::domain::users::model::DirectoryUser {
            id: 1,
            username: "system".to_string(),
            name: Some("System".to_string()),
            avatar_template: "/user_avatar/meta.discourse.org/system/{size}/1.png".to_string(),
            title: None,
        },
    };
    discourse_indexer::domain::users::repo::upsert_user(&db, "mock.instance", &seed_user)
        .await
        .expect("seed user for post FK");

    // Categories endpoint returns empty (sync will no-op categories)
    let _m_cat0 = server
        .mock("GET", "/categories.json?include_subcategories=true&page=0")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"category_list":{"categories":[]}}"#)
        .create_async()
        .await;

    // Users directory empty -> break early
    let _m_users0 = server
        .mock("GET", "/directory_items.json?page=0&order=desc&period=all")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"directory_items":[]}"#)
        .create_async()
        .await;

    // Topics page 0 with one topic
    let topics_body = r#"{
        "topic_list": {
            "per_page": 30,
            "topics": [{
                "id": 123,
                "title": "Welcome",
                "fancy_title": "Welcome",
                "slug": "welcome",
                "posts_count": 1,
                "reply_count": 0,
                "highest_post_number": 1,
                "image_url": null,
                "created_at": "2024-08-01T00:00:00Z",
                "last_posted_at": "2024-08-01T00:00:00Z",
                "bumped": false,
                "bumped_at": "2024-08-01T00:00:00Z",
                "archetype": "regular",
                "unseen": null,
                "last_read_post_number": null,
                "unread_posts": null,
                "pinned": false,
                "unpinned": null,
                "visible": true,
                "closed": false,
                "archived": false,
                "notification_level": null,
                "bookmarked": null,
                "liked": null,
                "views": 10,
                "like_count": 0,
                "has_summary": false,
                "last_poster_username": "system",
                "category_id": 1,
                "op_like_count": 0,
                "pinned_globally": false,
                "featured_link": null
            }]
        }
    }"#;
    let _m_topics0 = server
        .mock("GET", "/latest.json?order=activity&page=0")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(topics_body)
        .create_async()
        .await;

    // Topics page 1 -> 404 ends pagination
    let _m_topics1 = server
        .mock("GET", "/latest.json?order=activity&page=1")
        .with_status(404)
        .create_async()
        .await;

    // Posts page uses page=1 for first page (adapter adds +1)
    let posts_body = r#"{
        "posts_count": 1,
        "post_stream": {"posts": [{
            "id": 11,
            "name": null,
            "username": "system",
            "avatar_template": "/user_avatar/meta.discourse.org/system/{size}/1.png",
            "created_at": "2024-08-01T00:00:00Z",
            "cooked": "<p>Hello</p>",
            "post_number": 1,
            "post_type": 1,
            "updated_at": "2024-08-01T00:00:00Z",
            "reply_count": 0,
            "reply_to_post_number": null,
            "quote_count": 0,
            "incoming_link_count": 0,
            "reads": 1,
            "readers_count": 1,
            "score": 1.0,
            "yours": false,
            "topic_id": 123,
            "topic_slug": "welcome",
            "display_username": null,
            "primary_group_name": null,
            "flair_name": null,
            "flair_url": null,
            "flair_bg_color": null,
            "flair_color": null,
            "version": 1,
            "can_edit": false,
            "can_delete": false,
            "can_recover": false,
            "can_wiki": false,
            "read": false,
            "user_title": null,
            "bookmarked": false,
            "moderator": false,
            "admin": false,
            "staff": false,
            "user_id": 1,
            "hidden": false,
            "trust_level": 1,
            "deleted_at": null,
            "user_deleted": false,
            "edit_reason": null,
            "can_view_edit_history": false,
            "wiki": false,
            "raw": null
        }]}
    }"#;
    let _m_posts0 = server
        .mock("GET", "/t/123.json?include_raw=true&page=1")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(posts_body)
        .create_async()
        .await;
    // Next posts page -> 404
    let _m_posts1 = server
        .mock("GET", "/t/123.json?include_raw=true&page=2")
        .with_status(404)
        .create_async()
        .await;

    // Likes for post id 11 -> empty
    let _m_likes = server
        .mock("GET", "/post_action_users.json?id=11&post_action_type_id=2")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"post_action_users":[],"total_rows_post_action_users":0}"#)
        .create_async()
        .await;

    let client = DiscourseClient::new(base);
    let conc = Concurrency::new(4, 2, 2, 2);
    // Use small caps
    sync::sync_recent(&db, &client, "mock.instance", 1, 1, &conc)
        .await
        .expect("sync_recent");

    let topics_count = discourse_topics::Entity::find().count(&db).await.unwrap();
    let posts_count = discourse_posts::Entity::find().count(&db).await.unwrap();
    assert_eq!(topics_count, 1);
    assert_eq!(posts_count, 1);
}
