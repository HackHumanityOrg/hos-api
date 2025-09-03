#[path = "../common/mod.rs"]
mod common;

use common::TestDatabase;
use discourse_indexer::discourse_api::DiscourseClient;
use discourse_indexer::domain::categories::{adapter, repo};
use entities::discourse_categories;
use rstest::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

const TEST_INSTANCE: &str = "meta.discourse.org";

#[fixture]
async fn test_db() -> TestDatabase {
    TestDatabase::new().await
}

/// Live + DB: fetch categories via adapter, persist, and verify upsert.
#[rstest]
#[tokio::test]
async fn test_domain_categories_fetch_and_persist(#[future] test_db: TestDatabase) {
    let test_db = test_db.await;
    let db = test_db.db();

    // Ensure instance exists for FKs
    discourse_indexer::domain::instances::repo::register_instance(&db, TEST_INSTANCE, "https://meta.discourse.org")
        .await
        .expect("register instance");

    let client = DiscourseClient::new("https://meta.discourse.org".to_string());
    let categories = adapter::fetch_categories_all(&client, 2)
        .await
        .expect("fetch categories");
    assert!(!categories.is_empty());

    // Persist a subset for speed
    for c in categories.iter().take(5) {
        repo::upsert_category(&db, TEST_INSTANCE, c)
            .await
            .expect("upsert category");
    }

    // Re-upsert idempotently
    for c in categories.iter().take(5) {
        repo::upsert_category(&db, TEST_INSTANCE, c)
            .await
            .expect("upsert category idempotent");
    }

    // Update a field locally and upsert to validate update path
    if let Some(first) = categories.first() {
        let mut updated = first.clone();
        updated.name = format!("{}{}", &updated.name, "_updated");
        repo::upsert_category(&db, TEST_INSTANCE, &updated)
            .await
            .expect("upsert category update");

        let stored = discourse_categories::Entity::find()
            .filter(discourse_categories::Column::Instance.eq(TEST_INSTANCE))
            .filter(discourse_categories::Column::ExternalId.eq(updated.id))
            .one(&db)
            .await
            .expect("fetch stored")
            .expect("row exists");
        assert_eq!(stored.name, updated.name);
    }
}
