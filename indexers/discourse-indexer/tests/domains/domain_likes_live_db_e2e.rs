#[path = "../common/mod.rs"]
mod common;

use common::TestDatabase;
use discourse_indexer::discourse_api::DiscourseClient;
use discourse_indexer::domain::{
    likes::{adapter as likes_adapter, repo as likes_repo},
    posts::adapter as posts_adapter,
    topics::adapter as topics_adapter,
};
use entities::discourse_post_likes;
use rstest::*;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

const TEST_INSTANCE: &str = "meta.discourse.org";

#[fixture]
async fn test_db() -> TestDatabase {
    TestDatabase::new().await
}

/// Live + DB: search until we find a post with likes, persist, and verify update path.
#[rstest]
#[tokio::test]
async fn test_domain_likes_fetch_and_persist(#[future] test_db: TestDatabase) {
    let test_db = test_db.await;
    let db = test_db.db();
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());

    // Search multiple pages of topics and posts until we find likes
    let mut found: Option<(
        i32,
        discourse_indexer::domain::likes::model::PostLikeResponse,
    )> = None;
    'outer: for page in 0..5 {
        let latest = match topics_adapter::fetch_topics_recent_page(&client, page).await {
            Ok(v) => v,
            Err(_) => continue,
        };
        for t in latest.topic_list.topics.into_iter().take(20) {
            for post_page in 0..=1 {
                let posts = match posts_adapter::fetch_posts_page(&client, t.id, post_page).await {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                for p in posts.post_stream.posts {
                    match likes_adapter::fetch_post_likes(&client, p.id).await {
                        Ok(likes) if !likes.post_action_users.is_empty() => {
                            found = Some((p.id, likes));
                            break 'outer;
                        }
                        _ => continue,
                    }
                }
            }
        }
    }

    assert!(
        found.is_some(),
        "No post with likes found after scanning recent topics"
    );
    let (post_id, likes) = found.unwrap();

    // Persist and verify update path
    likes_repo::upsert_post_likes_batch(&db, TEST_INSTANCE, post_id, &likes)
        .await
        .expect("upsert likes");

    // Update: rename first user locally and re-upsert
    let mut likes_updated = likes.clone();
    likes_updated.post_action_users[0]
        .username
        .push_str("_updated");
    likes_repo::upsert_post_likes_batch(&db, TEST_INSTANCE, post_id, &likes_updated)
        .await
        .expect("update likes");

    let stored = discourse_post_likes::Entity::find()
        .filter(discourse_post_likes::Column::Instance.eq(TEST_INSTANCE))
        .filter(discourse_post_likes::Column::ExternalDiscoursePostId.eq(post_id))
        .filter(
            discourse_post_likes::Column::ExternalUserId.eq(likes_updated.post_action_users[0].id),
        )
        .one(&db)
        .await
        .expect("fetch like")
        .expect("like exists");
    assert!(stored.username.ends_with("_updated"));
}
