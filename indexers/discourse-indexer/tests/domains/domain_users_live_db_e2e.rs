#[path = "../common/mod.rs"]
mod common;

use common::TestDatabase;
use discourse_indexer::discourse_api::DiscourseClient;
use discourse_indexer::domain::users::{adapter, repo};
use entities::discourse_users;
use rstest::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

const TEST_INSTANCE: &str = "meta.discourse.org";

#[fixture]
async fn test_db() -> TestDatabase {
    TestDatabase::new().await
}

/// Live + DB: fetch directory users via adapter, persist, and verify update path.
#[rstest]
#[cfg_attr(not(feature = "live_http"), ignore)]
#[tokio::test]
async fn test_domain_users_fetch_and_persist(#[future] test_db: TestDatabase) {
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

    let users = adapter::fetch_users_directory(&client, true, 1, 0)
        .await
        .expect("users");
    assert!(!users.is_empty());

    let mut item = users[0].clone();
    repo::upsert_user(&db, TEST_INSTANCE, &item)
        .await
        .expect("upsert user");

    // Update locally and re-upsert
    item.likes_received += 1;
    repo::upsert_user(&db, TEST_INSTANCE, &item)
        .await
        .expect("update user");

    let stored = discourse_users::Entity::find()
        .filter(discourse_users::Column::Instance.eq(TEST_INSTANCE))
        .filter(discourse_users::Column::ExternalId.eq(item.user.id))
        .one(&db)
        .await
        .expect("fetch user")
        .expect("user exists");
    assert_eq!(stored.likes_received, item.likes_received);
}
