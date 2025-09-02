#[path = "../common/mod.rs"]
mod common;

use common::TestDatabase;
use discourse_indexer::discourse_api::DiscourseClient;
use discourse_indexer::domain::{
    posts::adapter as posts_adapter,
    revisions::{adapter as revisions_adapter, repo as revisions_repo},
    topics::adapter as topics_adapter,
};
use entities::discourse_post_revisions;
use rstest::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

const TEST_INSTANCE: &str = "meta.discourse.org";

#[fixture]
async fn test_db() -> TestDatabase {
    TestDatabase::new().await
}

/// Live + DB: find a post with edit history, fetch a revision, persist, and verify update path.
#[rstest]
#[tokio::test]
async fn test_domain_revisions_fetch_and_persist(#[future] test_db: TestDatabase) {
    let test_db = test_db.await;
    let db = test_db.db();
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());

    // Find a post with revisions and ensure the revision endpoint succeeds
    let mut found: Option<(i32, i32)> = None; // (post_id, version)
    'outer: for page in 0..5 {
        let latest = match topics_adapter::fetch_topics_recent_page(&client, page).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        for t in latest.topic_list.topics.into_iter().take(20) {
            // Try first 2 pages of posts for the topic
            for post_page in 0..=1 {
                let posts = match posts_adapter::fetch_posts_page(&client, t.id, post_page).await {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                for p in posts.post_stream.posts {
                    if p.can_view_edit_history && p.version > 1 {
                        // Try revision versions 2..=p.version with a timeout
                        let max_ver = std::cmp::min(p.version, 5);
                        for ver in 2..=max_ver {
                            match tokio::time::timeout(
                                std::time::Duration::from_secs(10),
                                revisions_adapter::fetch_post_revision(&client, p.id, ver),
                            )
                            .await
                            {
                                Ok(Ok(_)) => {
                                    found = Some((p.id, ver));
                                    break 'outer;
                                }
                                _ => continue,
                            }
                        }
                    }
                }
            }
        }
    }

    assert!(
        found.is_some(),
        "No post with accessible revisions found after scanning recent topics"
    );
    if let Some((post_id, version)) = found {
        let mut rev = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            revisions_adapter::fetch_post_revision(&client, post_id, version),
        )
        .await
        .expect("timeout fetching revision")
        .expect("revision fetch");
        revisions_repo::upsert_revision(&db, TEST_INSTANCE, post_id, &rev)
            .await
            .expect("upsert rev");

        // Update locally and re-upsert
        rev.username.push_str("_updated");
        revisions_repo::upsert_revision(&db, TEST_INSTANCE, post_id, &rev)
            .await
            .expect("update rev");

        let stored = discourse_post_revisions::Entity::find()
            .filter(discourse_post_revisions::Column::Instance.eq(TEST_INSTANCE))
            .filter(discourse_post_revisions::Column::ExternalPostId.eq(post_id))
            .filter(discourse_post_revisions::Column::CurrentVersion.eq(version))
            .one(&db)
            .await
            .expect("fetch rev")
            .expect("rev exists");
        assert!(stored.username.ends_with("_updated"));
    }
}
