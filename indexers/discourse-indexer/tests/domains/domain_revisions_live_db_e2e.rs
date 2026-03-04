use anyhow::Result;
use discourse_indexer::domain::categories;
use discourse_indexer::domain::instances;
use discourse_indexer::domain::posts;
use discourse_indexer::domain::revisions::{adapter, repo};
use discourse_indexer::domain::topics::{self, model::Topic};
use entities::{
    discourse::discourse_post_revisions, discourse::discourse_posts,
    prelude::DiscoursePostRevisions, prelude::DiscoursePosts,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use test_support::{init_test_tracing, TestDatabase};

#[path = "../support/mod.rs"]
mod support;
use support::FixtureFetcher;

const INSTANCE_KEY: &str = "meta";
const INSTANCE_URL: &str = "https://meta.discourse.org";

#[tokio::test]
async fn fetch_post_revision_parses_fixture_and_extracts_markdown() -> Result<()> {
    let fetcher = FixtureFetcher::new(&[(
        "/posts/326680/revisions/2.json",
        "meta/post_326680_revision_2.json",
    )])?;

    let revision = adapter::fetch_post_revision(&fetcher, 326680, 2).await?;
    assert_eq!(revision.current_revision, 2);
    assert_eq!(revision.username, "angus");
    let before_raw = revision.get_raw_markdown_before();
    let after_raw = revision.get_raw_markdown_after();
    assert!(
        before_raw.contains("Repo: https://github.com/angusmcleod/discourse-events"),
        "before markdown should include repo summary"
    );
    assert!(
        before_raw.contains("Dev notes:"),
        "before markdown should capture original heading"
    );
    assert!(
        !before_raw.contains("<del>"),
        "before markdown should not include diff deletion tags"
    );
    assert!(
        after_raw.contains("(when it is released)"),
        "after markdown should include inserted clarification"
    );
    assert!(
        after_raw.contains("More notes:"),
        "after markdown should capture updated heading"
    );
    assert!(
        !after_raw.contains("<ins>"),
        "after markdown should not include diff insertion tags"
    );
    let title_changes = revision
        .title_changes
        .as_ref()
        .expect("title changes present");
    let title_before = title_changes.get_raw_title_before();
    let title_after = title_changes.get_raw_title_after();
    assert_eq!(title_before, "Events Plugin :calendar:");
    assert_eq!(title_after, "Events Plugin :calendar:");

    Ok(())
}

#[tokio::test]
async fn upsert_revision_validates_all_columns() -> Result<()> {
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
    topics.push(topic_seed);
    topics::repo::upsert_topics_bulk(conn.as_ref(), INSTANCE_KEY, &topics).await?;

    let posts_fetcher = FixtureFetcher::new(&[(
        "/t/69776.json?include_raw=true",
        "meta/topic_69776_posts_page_0.json",
    )])?;
    let posts = posts::adapter::fetch_posts_page(&posts_fetcher, 69776, 0)
        .await?
        .post_stream
        .posts;
    posts::repo::upsert_posts_bulk(conn.as_ref(), INSTANCE_KEY, &posts).await?;

    let revisions_fetcher = FixtureFetcher::new(&[(
        "/posts/326680/revisions/2.json",
        "meta/post_326680_revision_2.json",
    )])?;
    let revision = adapter::fetch_post_revision(&revisions_fetcher, 326680, 2).await?;
    let raw_before = revision.get_raw_markdown_before();
    let raw_after = revision.get_raw_markdown_after();
    let title_before = revision
        .title_changes
        .as_ref()
        .map(|tc| tc.get_raw_title_before());
    let title_after = revision
        .title_changes
        .as_ref()
        .map(|tc| tc.get_raw_title_after());
    let body_changes = revision
        .body_changes
        .as_ref()
        .map(serde_json::to_value)
        .transpose()?;
    let title_changes = revision
        .title_changes
        .as_ref()
        .map(serde_json::to_value)
        .transpose()?;

    repo::upsert_revision(conn.as_ref(), INSTANCE_KEY, 326680, &revision).await?;

    let stored = DiscoursePostRevisions::find()
        .filter(discourse_post_revisions::Column::Instance.eq(INSTANCE_KEY))
        .filter(discourse_post_revisions::Column::ExternalPostId.eq(326680))
        .filter(discourse_post_revisions::Column::Version.eq(2))
        .one(conn.as_ref())
        .await?
        .expect("revision stored");

    assert_eq!(stored.instance, INSTANCE_KEY);
    assert_eq!(stored.external_post_id, 326680);
    assert_eq!(stored.version, revision.current_version);
    // API provides millisecond precision, which round-trips perfectly through Postgres (microsecond precision)
    assert_eq!(stored.created_at, revision.created_at.naive_utc());
    assert_eq!(stored.username, revision.username);
    assert_eq!(
        stored.edit_reason.as_deref(),
        revision.edit_reason.as_deref()
    );
    assert_eq!(stored.raw_body_before.as_deref(), Some(raw_before.as_str()));
    assert_eq!(stored.raw_body_after.as_deref(), Some(raw_after.as_str()));
    assert_eq!(stored.raw_title_before.as_deref(), title_before.as_deref());
    assert_eq!(stored.raw_title_after.as_deref(), title_after.as_deref());
    assert_eq!(stored.body_changes, body_changes);
    assert_eq!(stored.title_changes, title_changes);

    let post = DiscoursePosts::find()
        .filter(discourse_posts::Column::Instance.eq(INSTANCE_KEY))
        .filter(discourse_posts::Column::ExternalId.eq(326680))
        .one(conn.as_ref())
        .await?;
    assert!(post.is_some());

    Ok(())
}
