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

    let topics = topics_adapter::fetch_topics_recent_page(&client, 0)
        .await
        .expect("topics");
    let topic_id = topics.topic_list.topics[0].id;

    let pr = posts_adapter::fetch_posts_page(&client, topic_id, 0)
        .await
        .expect("posts");
    assert!(!pr.post_stream.posts.is_empty());

    // Persist a subset
    let mut posts: Vec<_> = pr.post_stream.posts.iter().cloned().take(3).collect();
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
