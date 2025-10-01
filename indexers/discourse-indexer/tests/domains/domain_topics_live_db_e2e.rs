use anyhow::Result;
use discourse_indexer::domain::categories;
use discourse_indexer::domain::instances;
use discourse_indexer::domain::topics::{adapter, repo};
use entities::{discourse::discourse_topics, prelude::DiscourseTopics};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use std::collections::HashMap;
use test_support::{init_test_tracing, TestDatabase};

#[path = "../support/mod.rs"]
mod support;
use support::FixtureFetcher;

const INSTANCE_KEY: &str = "meta";
const INSTANCE_URL: &str = "https://meta.discourse.org";
const RECENT_TOPIC_ID: i32 = 1;
const RECENT_TOPIC_SLUG: &str = "new-to-discourse-start-here";

#[tokio::test]
async fn fetch_topics_recent_page_parses_fixture() -> Result<()> {
    let fetcher =
        FixtureFetcher::new(&[("/latest.json?page=0", "meta/latest_activity_page_0.json")])?;

    let response = adapter::fetch_topics_recent_page(&fetcher, 0).await?;

    assert_eq!(response.topic_list.per_page, Some(30));
    assert_eq!(response.topic_list.topics.len(), 30);

    let topic = &response.topic_list.topics[0];
    assert_eq!(topic.id, RECENT_TOPIC_ID);
    assert_eq!(topic.slug, RECENT_TOPIC_SLUG);
    assert!(topic.bumped.unwrap_or(false));

    Ok(())
}

#[tokio::test]
async fn topics_upsert_checks_all_columns() -> Result<()> {
    init_test_tracing();
    let db = TestDatabase::new().await;
    let conn = db.db();

    instances::repo::register_instance(conn.as_ref(), INSTANCE_KEY, INSTANCE_URL).await?;

    let categories_fetcher = FixtureFetcher::new(&[(
        "/categories.json?include_subcategories=true&page=0",
        "meta/categories_page_0.json",
    )])?;
    let categories = categories::adapter::fetch_categories_all(&categories_fetcher, 0).await?;
    for category in &categories {
        categories::repo::upsert_category(conn.as_ref(), INSTANCE_KEY, category).await?;
    }

    let fetcher =
        FixtureFetcher::new(&[("/latest.json?page=0", "meta/latest_activity_page_0.json")])?;
    let topics = adapter::fetch_topics_recent_page(&fetcher, 0)
        .await?
        .topic_list
        .topics;

    repo::upsert_topics_bulk(conn.as_ref(), INSTANCE_KEY, &topics).await?;

    let stored = DiscourseTopics::find()
        .filter(discourse_topics::Column::Instance.eq(INSTANCE_KEY))
        .order_by_asc(discourse_topics::Column::ExternalId)
        .all(conn.as_ref())
        .await?;

    assert_eq!(stored.len(), topics.len());

    let expected: HashMap<i32, _> = topics.iter().map(|t| (t.id, t)).collect();

    for row in stored {
        let src = expected
            .get(&row.external_id)
            .expect("topic fixture present");
        assert_eq!(row.instance, INSTANCE_KEY);
        assert_eq!(row.external_id, src.id);
        assert_eq!(row.title, src.title);
        assert_eq!(
            row.fancy_title,
            src.fancy_title.clone().unwrap_or_else(|| src.title.clone())
        );
        assert_eq!(row.slug, src.slug);
        assert_eq!(row.posts_count, src.posts_count);
        assert_eq!(row.reply_count, src.reply_count);
        // API provides millisecond precision, which round-trips perfectly through Postgres (microsecond precision)
        assert_eq!(row.created_at, src.created_at.naive_utc());
        assert_eq!(row.last_posted_at, src.last_posted_at.naive_utc());
        assert_eq!(row.bumped_at, src.bumped_at.naive_utc());
        assert_eq!(row.pinned, src.pinned.unwrap_or(false));
        assert_eq!(row.pinned_globally, src.pinned_globally.unwrap_or(false));
        assert_eq!(row.visible, src.visible.unwrap_or(true));
        assert_eq!(row.closed, src.closed.unwrap_or(false));
        assert_eq!(row.archived, src.archived.unwrap_or(false));
        assert_eq!(row.views, src.views);
        assert_eq!(row.like_count, src.like_count);
        assert_eq!(row.external_category_id, src.category_id);
    }

    repo::upsert_topics_bulk(conn.as_ref(), INSTANCE_KEY, &topics).await?;
    let stored_second = DiscourseTopics::find()
        .filter(discourse_topics::Column::Instance.eq(INSTANCE_KEY))
        .order_by_asc(discourse_topics::Column::ExternalId)
        .all(conn.as_ref())
        .await?;
    assert_eq!(stored_second.len(), topics.len());

    let mut topics_updated = topics.clone();
    if let Some(first_topic) = topics_updated.first_mut() {
        first_topic.views += 1;
        first_topic.like_count += 1;
    }
    repo::upsert_topics_bulk(conn.as_ref(), INSTANCE_KEY, &topics_updated).await?;

    let stored_after_update = DiscourseTopics::find()
        .filter(discourse_topics::Column::Instance.eq(INSTANCE_KEY))
        .order_by_asc(discourse_topics::Column::ExternalId)
        .all(conn.as_ref())
        .await?;
    assert_eq!(stored_after_update.len(), topics.len());

    if let Some(first_topic) = topics_updated.first() {
        let stored_first = stored_after_update
            .iter()
            .find(|row| row.external_id == first_topic.id)
            .expect("updated topic stored");
        assert_eq!(stored_first.views, first_topic.views);
        assert_eq!(stored_first.like_count, first_topic.like_count);
    }

    Ok(())
}
