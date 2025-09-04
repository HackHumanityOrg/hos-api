#[path = "../common/mod.rs"]
mod common;

use common::TestDatabase;
use discourse_indexer::discourse_api::DiscourseClient;
use discourse_indexer::domain::{
    posts::{adapter as posts_adapter, repo as posts_repo},
    topics::adapter as topics_adapter,
};
use entities::discourse_posts;
use rstest::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

const TEST_INSTANCE: &str = "meta.discourse.org";

#[fixture]
async fn test_db() -> TestDatabase {
    TestDatabase::new().await
}

/// Live + DB: fetch posts for a topic via adapter, persist, and verify update path.
#[rstest]
#[tokio::test]
async fn test_domain_posts_fetch_and_persist(#[future] test_db: TestDatabase) {
    let test_db = test_db.await;
    let db = test_db.db();
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());

    discourse_indexer::domain::instances::repo::register_instance(
        &db,
        TEST_INSTANCE,
        "https://meta.discourse.org",
    )
    .await
    .expect("register instance");

    // Ensure categories exist to satisfy Topic FK
    let cats = discourse_indexer::domain::categories::adapter::fetch_categories_all(&client, 2)
        .await
        .expect("fetch categories for topic FK");
    for c in cats.iter() {
        discourse_indexer::domain::categories::repo::upsert_category(&db, TEST_INSTANCE, c)
            .await
            .expect("upsert category");
    }

    // Fetch directory users (several pages) and build a lookup by id for FK
    let dir_users =
        discourse_indexer::domain::users::adapter::fetch_users_directory(&client, true, 50, 50)
            .await
            .expect("fetch directory users");
    let mut user_by_id = std::collections::HashMap::new();
    for it in dir_users.iter() {
        user_by_id.insert(it.user.id, it.clone());
    }

    // Search topics and post pages for posts whose authors are in directory
    let mut chosen_topic: Option<discourse_indexer::domain::topics::model::Topic> = None;
    let mut posts: Vec<discourse_indexer::domain::posts::model::Post> = Vec::new();
    'outer: for tpage in 0..3 {
        let topics = topics_adapter::fetch_topics_recent_page(&client, tpage)
            .await
            .expect("topics");
        for t in topics.topic_list.topics.iter().cloned().take(10) {
            for ppage in 0..2 {
                let pr = posts_adapter::fetch_posts_page(&client, t.id, ppage)
                    .await
                    .unwrap_or_else(|_| discourse_indexer::domain::posts::model::PostResponse {
                        post_stream: discourse_indexer::domain::posts::model::PostStream {
                            posts: vec![],
                        },
                        posts_count: None,
                        id: None,
                        title: None,
                        fancy_title: None,
                        created_at: None,
                    });
                let mut filtered: Vec<_> = pr
                    .post_stream
                    .posts
                    .into_iter()
                    .filter(|p| user_by_id.contains_key(&p.user_id))
                    .take(3)
                    .collect();
                if !filtered.is_empty() {
                    chosen_topic = Some(t.clone());
                    posts.append(&mut filtered);
                    break 'outer;
                }
            }
        }
    }

    assert!(chosen_topic.is_some(), "No suitable topic found with posts whose authors are present in directory across scanned pages");
    let topic = chosen_topic.unwrap();

    // Upsert the chosen topic
    discourse_indexer::domain::topics::repo::upsert_topics_bulk(
        &db,
        TEST_INSTANCE,
        &[topic.clone()],
    )
    .await
    .expect("upsert topic for posts FK");

    // Upsert required users first
    for p in posts.iter() {
        if let Some(item) = user_by_id.get(&p.user_id) {
            discourse_indexer::domain::users::repo::upsert_user(&db, TEST_INSTANCE, item)
                .await
                .expect("upsert author user");
        }
    }
    // Ensure minimal users exist for FK (author of each post)
    for p in posts.iter() {
        let minimal_user = discourse_indexer::domain::users::model::DirectoryItem {
            id: 0,
            likes_received: 0,
            likes_given: 0,
            topics_entered: 0,
            topic_count: 0,
            post_count: 0,
            posts_read: 0,
            days_visited: 0,
            user: discourse_indexer::domain::users::model::DirectoryUser {
                id: p.user_id,
                username: p.username.clone(),
                name: Some(
                    p.display_username
                        .clone()
                        .unwrap_or_else(|| p.username.clone()),
                ),
                avatar_template: p.avatar_template.clone(),
                title: None,
            },
        };
        discourse_indexer::domain::users::repo::upsert_user(&db, TEST_INSTANCE, &minimal_user)
            .await
            .expect("upsert minimal user for post FK");
    }
    posts_repo::upsert_posts_bulk(&db, TEST_INSTANCE, &posts)
        .await
        .expect("upsert posts");

    // Update one locally and re-upsert
    posts[0].version += 1;
    posts[0].cooked.push_str("<!-- test update -->");
    posts_repo::upsert_posts_bulk(&db, TEST_INSTANCE, &[posts[0].clone()])
        .await
        .expect("update post");

    let stored = discourse_posts::Entity::find()
        .filter(discourse_posts::Column::Instance.eq(TEST_INSTANCE))
        .filter(discourse_posts::Column::ExternalId.eq(posts[0].id))
        .one(&db)
        .await
        .expect("fetch post")
        .expect("post exists");
    assert_eq!(stored.version, posts[0].version);
    assert!(stored.cooked.contains("test update"));
}
