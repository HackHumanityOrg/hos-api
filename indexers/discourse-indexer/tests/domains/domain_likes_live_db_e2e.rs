use anyhow::Result;
use chrono::{Timelike, Utc};
use discourse_indexer::domain::categories;
use discourse_indexer::domain::instances;
use discourse_indexer::domain::likes::{adapter, repo};
use discourse_indexer::domain::posts;
use discourse_indexer::domain::topics::{self, model::Topic};
use entities::{
    discourse_post_likes, discourse_posts, discourse_users, prelude::DiscoursePostLikes,
    prelude::DiscoursePosts, prelude::DiscourseUsers,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use std::collections::HashMap;
use test_support::{init_test_tracing, TestDatabase};

#[path = "../support/mod.rs"]
mod support;
use support::FixtureFetcher;

const INSTANCE_KEY: &str = "meta";
const INSTANCE_URL: &str = "https://meta.discourse.org";
const TOPIC_ID: i32 = 69776;
const POST_ID: i32 = 326680;

#[tokio::test]
async fn fetch_post_likes_parses_fixture() -> Result<()> {
    let fetcher = FixtureFetcher::new(&[(
        "/post_action_users.json?id=326680&post_action_type_id=2",
        "meta/post_326680_likes.json",
    )])?;

    let likes = adapter::fetch_post_likes(&fetcher, POST_ID).await?;
    assert!(
        likes.len() > 50,
        "expected at least 50 likes for fixture post"
    );
    let first = likes.first().expect("likes list non-empty");
    assert_eq!(first.username_lower, first.username.to_lowercase());

    Ok(())
}

#[tokio::test]
async fn upsert_post_likes_validates_columns_and_users() -> Result<()> {
    init_test_tracing();
    let db = TestDatabase::new().await;
    let conn = db.db();

    instances::repo::register_instance(conn.as_ref(), INSTANCE_KEY, INSTANCE_URL).await?;

    // Seed FK dependencies
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
        "/t/69776.json?page=0&include_raw=true",
        "meta/topic_69776_posts_page_0.json",
    )])?;
    let posts = posts::adapter::fetch_posts_page(&posts_fetcher, TOPIC_ID, 0)
        .await?
        .post_stream
        .posts;
    posts::repo::upsert_posts_bulk(conn.as_ref(), INSTANCE_KEY, &posts).await?;

    let likes_fetcher = FixtureFetcher::new(&[(
        "/post_action_users.json?id=326680&post_action_type_id=2",
        "meta/post_326680_likes.json",
    )])?;
    let likes = adapter::fetch_post_likes(&likes_fetcher, POST_ID).await?;

    let before = Utc::now().with_nanosecond(0).expect("valid UTC timestamp");
    repo::upsert_post_likes_batch(conn.as_ref(), INSTANCE_KEY, POST_ID, &likes).await?;
    let after = Utc::now().with_nanosecond(0).expect("valid UTC timestamp");

    let stored_likes = DiscoursePostLikes::find()
        .filter(discourse_post_likes::Column::Instance.eq(INSTANCE_KEY))
        .filter(discourse_post_likes::Column::ExternalPostId.eq(POST_ID))
        .all(conn.as_ref())
        .await?;
    assert_eq!(stored_likes.len(), likes.len());

    let expected_like_users: HashMap<i32, _> = likes.iter().map(|u| (u.id, u)).collect();

    let created_at_by_user: HashMap<_, _> = stored_likes
        .iter()
        .map(|row| (row.external_user_id, row.created_at))
        .collect();

    for row in stored_likes {
        assert_eq!(row.instance, INSTANCE_KEY);
        assert_eq!(row.external_post_id, POST_ID);
        assert!(expected_like_users.contains_key(&row.external_user_id));
        assert!(row.created_at >= before);
        assert!(row.created_at <= after);
    }

    // Re-run upsert to ensure idempotency and that timestamps advance monotonically.
    repo::upsert_post_likes_batch(conn.as_ref(), INSTANCE_KEY, POST_ID, &likes).await?;

    let stored_likes_second = DiscoursePostLikes::find()
        .filter(discourse_post_likes::Column::Instance.eq(INSTANCE_KEY))
        .filter(discourse_post_likes::Column::ExternalPostId.eq(POST_ID))
        .all(conn.as_ref())
        .await?;
    assert_eq!(stored_likes_second.len(), likes.len());

    for row in stored_likes_second {
        let previous_created = created_at_by_user
            .get(&row.external_user_id)
            .expect("initial like timestamp");
        assert!(
            row.created_at >= *previous_created,
            "created_at should not regress"
        );
    }

    // Ensure users created via likes path have expected columns populated
    let tracked_user_ids = likes.iter().take(3).map(|u| u.id).collect::<Vec<_>>();
    let stored_users = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(INSTANCE_KEY))
        .filter(discourse_users::Column::ExternalId.is_in(tracked_user_ids.clone()))
        .order_by_asc(discourse_users::Column::ExternalId)
        .all(conn.as_ref())
        .await?;

    assert_eq!(stored_users.len(), tracked_user_ids.len());

    for row in stored_users {
        let src = expected_like_users
            .get(&row.external_id)
            .expect("like user present");
        assert_eq!(row.instance, INSTANCE_KEY);
        assert_eq!(row.username, src.username);
        assert_eq!(row.name.as_deref(), src.name.as_deref());
        assert_eq!(row.avatar_template, src.avatar_template);
        assert_eq!(row.title, None);
        assert_eq!(row.likes_received, None);
        assert_eq!(row.likes_given, None);
        assert_eq!(row.topics_entered, None);
        assert_eq!(row.topic_count, None);
        assert_eq!(row.post_count, None);
        assert_eq!(row.posts_read, None);
        assert_eq!(row.days_visited, None);
    }

    // Ensure the post row still exists to satisfy relation integrity
    let post = DiscoursePosts::find()
        .filter(discourse_posts::Column::Instance.eq(INSTANCE_KEY))
        .filter(discourse_posts::Column::ExternalId.eq(POST_ID))
        .one(conn.as_ref())
        .await?;
    assert!(post.is_some());

    Ok(())
}
