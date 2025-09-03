#[path = "../common/mod.rs"]
mod common;

use common::TestDatabase;
use discourse_indexer::discourse_api::DiscourseClient;
use discourse_indexer::domain::topics::{adapter, repo};
use entities::discourse_topics;
use rstest::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

const TEST_INSTANCE: &str = "meta.discourse.org";

#[fixture]
async fn test_db() -> TestDatabase {
    TestDatabase::new().await
}

/// Live + DB: fetch topics from latest page, persist bulk, and verify upsert.
#[rstest]
#[tokio::test]
async fn test_domain_topics_fetch_and_persist(#[future] test_db: TestDatabase) {
    let test_db = test_db.await;
    let db = test_db.db();
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());

    discourse_indexer::domain::instances::repo::register_instance(&db, TEST_INSTANCE, "https://meta.discourse.org")
        .await
        .expect("register instance");

    let resp = adapter::fetch_topics_recent_page(&client, 0)
        .await
        .expect("fetch topics");
    assert!(!resp.topic_list.topics.is_empty());

    // Ensure categories exist to satisfy Topics→Categories FK
    let cats = discourse_indexer::domain::categories::adapter::fetch_categories_all(&client, 2)
        .await
        .expect("fetch categories for topic FK");
    for c in cats.iter() {
        discourse_indexer::domain::categories::repo::upsert_category(&db, TEST_INSTANCE, c)
            .await
            .expect("upsert category");
    }

    // Persist a subset
    let subset: Vec<_> = resp.topic_list.topics.iter().cloned().take(5).collect();
    repo::upsert_topics_bulk(&db, TEST_INSTANCE, &subset)
        .await
        .expect("upsert topics");

    // Idempotent re-upsert
    repo::upsert_topics_bulk(&db, TEST_INSTANCE, &subset)
        .await
        .expect("upsert topics again");

    // Update one topic locally to validate update columns
    let mut t = subset[0].clone();
    t.title = format!("{}{}", t.title, " (updated)");
    repo::upsert_topics_bulk(&db, TEST_INSTANCE, &[t.clone()])
        .await
        .expect("update topic");

    let stored = discourse_topics::Entity::find()
        .filter(discourse_topics::Column::Instance.eq(TEST_INSTANCE))
        .filter(discourse_topics::Column::ExternalId.eq(t.id))
        .one(&db)
        .await
        .expect("fetch t")
        .expect("t exists");
    assert_eq!(stored.title, t.title);
}
