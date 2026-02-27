use anyhow::Result;
use discourse_indexer::domain::instances;
use discourse_indexer::domain::users::{adapter, repo};
use entities::{discourse::discourse_users, prelude::DiscourseUsers};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use std::collections::{HashMap, HashSet};
use test_support::{init_test_tracing, TestDatabase};

#[path = "../support/mod.rs"]
mod support;
use support::FixtureFetcher;

const INSTANCE_KEY: &str = "meta";
const INSTANCE_URL: &str = "https://meta.discourse.org";

#[tokio::test]
async fn fetch_users_directory_paginates_until_empty_page() -> Result<()> {
    let fetcher = FixtureFetcher::new(&[
        (
            "/directory_items.json?period=all&order=username&page=0&asc=true",
            "meta/directory_items_page_0.json",
        ),
        (
            "/directory_items.json?period=all&order=username&page=1&asc=true",
            "meta/directory_items_page_1.json",
        ),
        (
            "/directory_items.json?period=all&order=username&page=2&asc=true",
            "meta/directory_items_page_2.json",
        ),
    ])?;

    let items = adapter::fetch_users_directory(&fetcher, false, 1, 5).await?;
    assert_eq!(items.len(), 100);
    let unique_ids: HashSet<i32> = items.iter().map(|item| item.user.id).collect();
    assert_eq!(unique_ids.len(), items.len());

    Ok(())
}

#[tokio::test]
async fn user_upsert_validates_all_columns() -> Result<()> {
    init_test_tracing();
    let db = TestDatabase::new().await;
    let conn = db.db();

    instances::repo::register_instance(conn.as_ref(), INSTANCE_KEY, INSTANCE_URL).await?;

    let fetcher = FixtureFetcher::new(&[(
        "/directory_items.json?period=all&order=username&page=0&asc=true",
        "meta/directory_items_page_0.json",
    )])?;
    let items = adapter::fetch_users_directory(&fetcher, false, 0, 1).await?;
    assert!(
        !items.is_empty(),
        "expected user directory items from fixture"
    );

    for item in &items {
        repo::upsert_user(conn.as_ref(), INSTANCE_KEY, item).await?;
    }

    let stored = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(INSTANCE_KEY))
        .order_by_asc(discourse_users::Column::ExternalId)
        .all(conn.as_ref())
        .await?;

    assert_eq!(stored.len(), items.len());

    let expected: HashMap<i32, _> = items.iter().map(|item| (item.user.id, item)).collect();

    for row in stored {
        let src = expected
            .get(&row.external_id)
            .expect("user fixture present");
        assert_eq!(row.instance, INSTANCE_KEY);
        assert_eq!(row.external_id, src.user.id);
        assert_eq!(row.username, src.user.username);
        assert_eq!(row.name.as_deref(), src.user.name.as_deref());
        assert_eq!(row.avatar_template, src.user.avatar_template);
        assert_eq!(row.title.as_deref(), src.user.title.as_deref());
        assert_eq!(row.likes_received, src.likes_received.map(|v| v as i64));
        assert_eq!(row.likes_given, src.likes_given.map(|v| v as i64));
        assert_eq!(row.topics_entered, src.topics_entered.map(|v| v as i64));
        assert_eq!(row.topic_count, src.topic_count.map(|v| v as i64));
        assert_eq!(row.post_count, src.post_count.map(|v| v as i64));
        assert_eq!(row.posts_read, src.posts_read.map(|v| v as i64));
        assert_eq!(row.days_visited, src.days_visited.map(|v| v as i64));
    }

    // Second upsert with the same data should not introduce duplicates.
    for item in &items {
        repo::upsert_user(conn.as_ref(), INSTANCE_KEY, item).await?;
    }
    let stored_again = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(INSTANCE_KEY))
        .order_by_asc(discourse_users::Column::ExternalId)
        .all(conn.as_ref())
        .await?;
    assert_eq!(stored_again.len(), items.len());

    // Modify a directory item to ensure updates propagate.
    if let Some(first_item) = items.first() {
        let mut updated_item = first_item.clone();
        updated_item.likes_received = updated_item.likes_received.map(|v| v + 1);
        repo::upsert_user(conn.as_ref(), INSTANCE_KEY, &updated_item).await?;

        let stored_updated = DiscourseUsers::find()
            .filter(discourse_users::Column::Instance.eq(INSTANCE_KEY))
            .filter(discourse_users::Column::ExternalId.eq(updated_item.user.id))
            .one(conn.as_ref())
            .await?
            .expect("updated user stored");
        assert_eq!(
            stored_updated.likes_received,
            updated_item.likes_received.map(|v| v as i64)
        );
    }

    Ok(())
}
