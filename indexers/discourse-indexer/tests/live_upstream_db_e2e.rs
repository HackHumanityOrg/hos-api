use anyhow::{bail, Context, Result};
use discourse_indexer::discourse_api::DiscourseClient;
use discourse_indexer::domain::{categories, instances, likes, posts, revisions, topics, users};
use entities::{
    discourse::{
        discourse_categories, discourse_post_likes, discourse_post_revisions, discourse_posts,
        discourse_topics, discourse_users,
    },
    prelude::{
        DiscourseCategories, DiscoursePostLikes, DiscoursePostRevisions, DiscoursePosts,
        DiscourseTopics, DiscourseUsers,
    },
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter};
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use test_support::{init_test_tracing, TestDatabase};
use tracing::warn;

#[tokio::test]
#[ignore = "hits real Discourse APIs"]
async fn live_upstream_pipeline_uses_real_api_and_postgres_testcontainer() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();

    run_instance_with_retry(conn.clone(), "meta", "https://meta.discourse.org").await?;
    run_instance_with_retry(conn.clone(), "near", "https://gov.near.org").await?;

    Ok(())
}

async fn run_instance_with_retry(
    db: Arc<DatabaseConnection>,
    instance: &str,
    base_url: &str,
) -> Result<()> {
    for attempt in 1..=2 {
        match run_instance_once(db.clone(), instance, base_url).await {
            Ok(()) => return Ok(()),
            Err(err) if attempt == 1 => {
                warn!(
                    instance = instance,
                    base_url = base_url,
                    error = %err,
                    "Live upstream attempt failed; retrying once"
                );
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(err) => {
                return Err(err).context(format!(
                    "live upstream pipeline failed for instance {instance} ({base_url})"
                ));
            }
        }
    }

    bail!("unexpected retry loop termination")
}

async fn run_instance_once(
    db: Arc<DatabaseConnection>,
    instance: &str,
    base_url: &str,
) -> Result<()> {
    instances::repo::register_instance(db.as_ref(), instance, base_url).await?;
    let client = DiscourseClient::new(base_url.to_string());

    let categories_payload = with_timeout(
        "fetch categories",
        categories::adapter::fetch_categories_all(&client, 0),
    )
    .await?;
    if categories_payload.is_empty() {
        bail!("no categories returned from {base_url}");
    }
    for category in &categories_payload {
        categories::repo::upsert_category(db.as_ref(), instance, category).await?;
    }

    let category_ids: HashSet<i32> = categories_payload.iter().map(|c| c.id).collect();

    let recent_topics = with_timeout(
        "fetch recent topics",
        topics::adapter::fetch_topics_recent_page(&client, 0),
    )
    .await?
    .topic_list
    .topics;
    let selected_topics: Vec<_> = recent_topics
        .into_iter()
        .filter(|topic| category_ids.contains(&topic.category_id))
        .take(3)
        .collect();
    if selected_topics.is_empty() {
        bail!("no recent topics mapped to fetched categories for {base_url}");
    }
    topics::repo::upsert_topics_bulk(db.as_ref(), instance, &selected_topics).await?;

    let topic = selected_topics
        .first()
        .cloned()
        .context("selected topic missing")?;
    let topic_posts = with_timeout(
        "fetch topic posts",
        posts::adapter::fetch_posts_page(&client, topic.id, 0),
    )
    .await?
    .post_stream
    .posts;
    if topic_posts.is_empty() {
        bail!("no posts returned for topic {} from {base_url}", topic.id);
    }
    posts::repo::upsert_posts_bulk(db.as_ref(), instance, &topic_posts).await?;

    let directory_users = with_timeout(
        "fetch directory users",
        users::adapter::fetch_users_directory(&client, false, 0, 1),
    )
    .await?;
    if directory_users.is_empty() {
        bail!("directory endpoint returned no users for {base_url}");
    }
    for item in directory_users.iter().take(20) {
        users::repo::upsert_user(db.as_ref(), instance, item).await?;
    }

    let mut inserted_likes = false;
    for post in topic_posts
        .iter()
        .filter(|post| {
            post.actions_summary
                .iter()
                .any(|action| action.id == 2 && action.count > 0)
        })
        .take(3)
    {
        match with_timeout(
            "fetch post likes",
            likes::adapter::fetch_post_likes(&client, post.id),
        )
        .await
        {
            Ok(live_likes) if !live_likes.is_empty() => {
                likes::repo::upsert_post_likes_batch(db.as_ref(), instance, post.id, &live_likes)
                    .await?;
                inserted_likes = true;
                break;
            }
            Ok(_) => continue,
            Err(err) => {
                warn!(
                    instance = instance,
                    post_id = post.id,
                    error = %err,
                    "Skipping likes candidate after live fetch failure"
                );
            }
        }
    }

    let mut inserted_revision = false;
    for post in topic_posts
        .iter()
        .filter(|post| post.can_view_edit_history && post.version > 1)
        .take(3)
    {
        match with_timeout(
            "fetch post revision",
            revisions::adapter::fetch_post_revision(&client, post.id, 2),
        )
        .await
        {
            Ok(revision) => {
                revisions::repo::upsert_revision(db.as_ref(), instance, post.id, &revision).await?;
                inserted_revision = true;
                break;
            }
            Err(err) => {
                warn!(
                    instance = instance,
                    post_id = post.id,
                    error = %err,
                    "Skipping revision candidate after live fetch failure"
                );
            }
        }
    }

    let category_count = DiscourseCategories::find()
        .filter(discourse_categories::Column::Instance.eq(instance))
        .count(db.as_ref())
        .await?;
    let topic_count = DiscourseTopics::find()
        .filter(discourse_topics::Column::Instance.eq(instance))
        .count(db.as_ref())
        .await?;
    let post_count = DiscoursePosts::find()
        .filter(discourse_posts::Column::Instance.eq(instance))
        .count(db.as_ref())
        .await?;
    let user_count = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(instance))
        .count(db.as_ref())
        .await?;

    assert!(category_count > 0, "expected categories for {instance}");
    assert!(topic_count > 0, "expected topics for {instance}");
    assert!(post_count > 0, "expected posts for {instance}");
    assert!(user_count > 0, "expected users for {instance}");

    let stored_posts = DiscoursePosts::find()
        .filter(discourse_posts::Column::Instance.eq(instance))
        .all(db.as_ref())
        .await?;
    let stored_users = DiscourseUsers::find()
        .filter(discourse_users::Column::Instance.eq(instance))
        .all(db.as_ref())
        .await?;
    let stored_likes = DiscoursePostLikes::find()
        .filter(discourse_post_likes::Column::Instance.eq(instance))
        .all(db.as_ref())
        .await?;
    let stored_revisions = DiscoursePostRevisions::find()
        .filter(discourse_post_revisions::Column::Instance.eq(instance))
        .all(db.as_ref())
        .await?;

    let post_ids: HashSet<i32> = stored_posts.iter().map(|post| post.external_id).collect();
    let user_ids: HashSet<i32> = stored_users.iter().map(|user| user.external_id).collect();

    for post in &stored_posts {
        assert!(
            user_ids.contains(&post.external_user_id),
            "post {} should reference an indexed user",
            post.external_id
        );
    }

    for like in &stored_likes {
        assert!(
            post_ids.contains(&like.external_post_id),
            "like should reference indexed post {}",
            like.external_post_id
        );
        assert!(
            user_ids.contains(&like.external_user_id),
            "like should reference indexed user {}",
            like.external_user_id
        );
    }

    for revision in &stored_revisions {
        assert!(
            post_ids.contains(&revision.external_post_id),
            "revision should reference indexed post {}",
            revision.external_post_id
        );
    }

    if inserted_likes {
        assert!(
            !stored_likes.is_empty(),
            "likes were inserted but no likes are stored"
        );
    }

    if inserted_revision {
        assert!(
            !stored_revisions.is_empty(),
            "revision was inserted but no revisions are stored"
        );
    }

    Ok(())
}

async fn with_timeout<F, T>(label: &str, future: F) -> Result<T>
where
    F: Future<Output = Result<T>>,
{
    tokio::time::timeout(Duration::from_secs(90), future)
        .await
        .with_context(|| format!("{label} timed out"))?
}
