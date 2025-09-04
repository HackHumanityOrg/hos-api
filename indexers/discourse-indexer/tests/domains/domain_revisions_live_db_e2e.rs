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
#[cfg_attr(not(feature = "live_http"), ignore)]
#[tokio::test]
async fn test_domain_revisions_fetch_and_persist(#[future] test_db: TestDatabase) {
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

    // Pre-fetch directory users (many pages) to satisfy Posts→Users FK
    let dir_users =
        discourse_indexer::domain::users::adapter::fetch_users_directory(&client, true, 50, 50)
            .await
            .expect("fetch directory users");
    let mut dir_by_id = std::collections::HashMap::new();
    for it in dir_users.into_iter() {
        dir_by_id.insert(it.user.id, it);
    }

    // Find a post with revisions (and author present in directory), and ensure the revision endpoint succeeds
    let mut found: Option<(
        discourse_indexer::domain::topics::model::Topic,
        discourse_indexer::domain::posts::model::Post,
        i32,
    )> = None; // (topic, post, version)
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
                for p in posts.post_stream.posts.clone() {
                    if p.can_view_edit_history
                        && p.version > 1
                        && dir_by_id.contains_key(&p.user_id)
                    {
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
                                    found = Some((t.clone(), p.clone(), ver));
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
    if let Some((topic, post, version)) = found {
        // Ensure dependencies for FKs: categories -> topic -> user -> post
        let cats = discourse_indexer::domain::categories::adapter::fetch_categories_all(&client, 2)
            .await
            .expect("fetch categories");
        for c in cats.iter() {
            discourse_indexer::domain::categories::repo::upsert_category(&db, TEST_INSTANCE, c)
                .await
                .expect("upsert category");
        }
        discourse_indexer::domain::topics::repo::upsert_topics_bulk(
            &db,
            TEST_INSTANCE,
            &[topic.clone()],
        )
        .await
        .expect("upsert topic for revisions FK");

        // Upsert author user from pre-fetched directory map
        if let Some(item) = dir_by_id.get(&post.user_id) {
            discourse_indexer::domain::users::repo::upsert_user(&db, TEST_INSTANCE, item)
                .await
                .expect("upsert author user");
        }
        discourse_indexer::domain::posts::repo::upsert_posts_bulk(
            &db,
            TEST_INSTANCE,
            &[post.clone()],
        )
        .await
        .expect("upsert post for revisions FK");

        let mut rev = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            revisions_adapter::fetch_post_revision(&client, post.id, version),
        )
        .await
        .expect("timeout fetching revision")
        .expect("revision fetch");
        revisions_repo::upsert_revision(&db, TEST_INSTANCE, post.id, &rev)
            .await
            .expect("upsert rev");

        // Update locally and re-upsert
        rev.username.push_str("_updated");
        revisions_repo::upsert_revision(&db, TEST_INSTANCE, post.id, &rev)
            .await
            .expect("update rev");

        let stored = discourse_post_revisions::Entity::find()
            .filter(discourse_post_revisions::Column::Instance.eq(TEST_INSTANCE))
            .filter(discourse_post_revisions::Column::ExternalPostId.eq(post.id))
            .filter(discourse_post_revisions::Column::CurrentVersion.eq(version))
            .one(&db)
            .await
            .expect("fetch rev")
            .expect("rev exists");
        assert!(stored.username.ends_with("_updated"));
    }
}
