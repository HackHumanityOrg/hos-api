use anyhow::Result;
use chrono::Timelike;
use discourse_indexer::domain::categories;
use discourse_indexer::domain::instances;
use discourse_indexer::domain::posts::{adapter, repo};
use discourse_indexer::domain::topics::{self, model::Topic};
use entities::{
    discourse::discourse_posts, discourse::discourse_users, prelude::DiscoursePosts,
    prelude::DiscourseUsers,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use std::collections::{BTreeSet, HashMap};
use test_support::{init_test_tracing, TestDatabase};

#[path = "../support/mod.rs"]
mod support;
use support::FixtureFetcher;

const INSTANCE_KEY: &str = "meta";
const INSTANCE_URL: &str = "https://meta.discourse.org";
const TOPIC_ID: i32 = 69776;
const ROOT_POST_ID: i32 = 326680;

#[tokio::test]
async fn fetch_posts_page_parses_fixture() -> Result<()> {
    let fetcher = FixtureFetcher::new(&[(
        "/t/69776.json?include_raw=true",
        "meta/topic_69776_posts_page_0.json",
    )])?;

    let response = adapter::fetch_posts_page(&fetcher, TOPIC_ID, 0).await?;
    assert!(
        response.post_stream.posts.len() >= 20,
        "expected at least 20 posts on the first page"
    );

    let first = &response.post_stream.posts[0];
    assert_eq!(first.id, ROOT_POST_ID);
    assert_eq!(first.username, "angus");
    assert_eq!(first.version, 38);
    assert_eq!(first.actions_summary[0].id, 2);

    Ok(())
}

#[tokio::test]
async fn posts_upsert_checks_every_column() -> Result<()> {
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

    let topics_fetcher =
        FixtureFetcher::new(&[("/latest.json?page=0", "meta/latest_activity_page_0.json")])?;
    let mut topics = topics::adapter::fetch_topics_recent_page(&topics_fetcher, 0)
        .await?
        .topic_list
        .topics;
    let topic_seed: Topic = support::load_topic_from_fixture("meta/topic_69776_posts_page_0.json")?;
    topics.push(topic_seed.clone());
    topics::repo::upsert_topics_bulk(conn.as_ref(), INSTANCE_KEY, &topics).await?;

    let posts_fetcher = FixtureFetcher::new(&[(
        "/t/69776.json?include_raw=true",
        "meta/topic_69776_posts_page_0.json",
    )])?;
    let posts = adapter::fetch_posts_page(&posts_fetcher, TOPIC_ID, 0)
        .await?
        .post_stream
        .posts;

    repo::upsert_posts_bulk(conn.as_ref(), INSTANCE_KEY, &posts).await?;

    let stored_posts = DiscoursePosts::find()
        .filter(discourse_posts::Column::Instance.eq(INSTANCE_KEY))
        .order_by_asc(discourse_posts::Column::ExternalId)
        .all(conn.as_ref())
        .await?;
    assert_eq!(stored_posts.len(), posts.len());

    let expected_posts: HashMap<i32, _> = posts.iter().map(|p| (p.id, p)).collect();

    for row in &stored_posts {
        let src = expected_posts
            .get(&row.external_id)
            .expect("post fixture present");
        assert_eq!(row.instance, INSTANCE_KEY);
        assert_eq!(row.external_id, src.id);
        assert_eq!(row.name.as_deref(), src.name.as_deref());
        assert_eq!(row.username, src.username);
        let expected_created = src
            .created_at
            .with_nanosecond(0)
            .expect("valid created_at timestamp")
            .naive_utc();
        let expected_updated = src
            .updated_at
            .with_nanosecond(0)
            .expect("valid updated_at timestamp")
            .naive_utc();
        assert_eq!(row.created_at, expected_created);
        assert_eq!(row.post_number, src.post_number);
        assert_eq!(row.post_type, src.post_type);
        assert_eq!(row.updated_at, expected_updated);
        assert_eq!(row.reply_count, src.reply_count);
        assert_eq!(row.reply_to_post_number, src.reply_to_post_number);
        assert_eq!(row.quote_count, src.quote_count);
        assert_eq!(row.incoming_link_count, src.incoming_link_count);
        assert_eq!(row.reads, src.reads);
        assert_eq!(row.readers_count, src.readers_count);
        let score_delta = (row.score - src.score).abs();
        assert!(
            score_delta < 1e-6,
            "score mismatch: {} vs {}",
            row.score,
            src.score
        );
        assert_eq!(row.external_topic_id, src.topic_id);
        assert_eq!(row.topic_slug, src.topic_slug);
        assert_eq!(
            row.display_username.as_deref(),
            src.display_username.as_deref()
        );
        assert_eq!(
            row.primary_group_name.as_deref(),
            src.primary_group_name.as_deref()
        );
        assert_eq!(row.flair_name.as_deref(), src.flair_name.as_deref());
        assert_eq!(row.flair_url.as_deref(), src.flair_url.as_deref());
        assert_eq!(row.flair_bg_color.as_deref(), src.flair_bg_color.as_deref());
        assert_eq!(row.flair_color.as_deref(), src.flair_color.as_deref());
        assert_eq!(row.version, src.version);
        assert_eq!(row.external_user_id, src.user_id);
        assert_eq!(row.can_view_edit_history, src.can_view_edit_history);
        assert_eq!(row.deleted, src.deleted);
        assert_eq!(row.raw.as_deref(), src.raw.as_deref());
    }

    // Idempotent second run should not duplicate records.
    repo::upsert_posts_bulk(conn.as_ref(), INSTANCE_KEY, &posts).await?;
    let stored_after_second = DiscoursePosts::find()
        .filter(discourse_posts::Column::Instance.eq(INSTANCE_KEY))
        .order_by_asc(discourse_posts::Column::ExternalId)
        .all(conn.as_ref())
        .await?;
    assert_eq!(stored_after_second.len(), posts.len());

    // Modify a post payload to ensure updates propagate.
    let mut posts_updated = posts.clone();
    if let Some(first_post) = posts_updated.first_mut() {
        first_post.raw = Some("Updated raw body from test".to_string());
        first_post.display_username = Some("Updated Display".to_string());
    }
    repo::upsert_posts_bulk(conn.as_ref(), INSTANCE_KEY, &posts_updated).await?;

    let stored_after_update = DiscoursePosts::find()
        .filter(discourse_posts::Column::Instance.eq(INSTANCE_KEY))
        .order_by_asc(discourse_posts::Column::ExternalId)
        .all(conn.as_ref())
        .await?;
    assert_eq!(stored_after_update.len(), posts.len());

    if let Some(first_post) = posts_updated.first() {
        let stored_first = stored_after_update
            .iter()
            .find(|row| row.external_id == first_post.id)
            .expect("updated post stored");
        assert_eq!(stored_first.raw.as_deref(), first_post.raw.as_deref());
        assert_eq!(
            stored_first.display_username.as_deref(),
            first_post.display_username.as_deref()
        );
    }

    let stored_users = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(INSTANCE_KEY))
        .order_by_asc(discourse_users::Column::ExternalId)
        .all(conn.as_ref())
        .await?;
    let unique_user_ids: BTreeSet<i32> = posts.iter().map(|p| p.user_id).collect();
    assert_eq!(
        stored_users.len(),
        unique_user_ids.len(),
        "post upsert should insert all referenced users"
    );

    for user_row in stored_users {
        let src_post = posts
            .iter()
            .find(|p| p.user_id == user_row.external_id)
            .expect("post provides user");
        assert_eq!(user_row.instance, INSTANCE_KEY);
        assert_eq!(user_row.external_id, src_post.user_id);
        assert_eq!(user_row.username, src_post.username);
        assert_eq!(user_row.name.as_deref(), src_post.name.as_deref());
        assert_eq!(user_row.avatar_template, src_post.avatar_template);
        assert_eq!(user_row.title, None);
        assert_eq!(user_row.likes_received, None);
        assert_eq!(user_row.likes_given, None);
        assert_eq!(user_row.topics_entered, None);
        assert_eq!(user_row.topic_count, None);
        assert_eq!(user_row.post_count, None);
        assert_eq!(user_row.posts_read, None);
        assert_eq!(user_row.days_visited, None);
    }

    Ok(())
}
