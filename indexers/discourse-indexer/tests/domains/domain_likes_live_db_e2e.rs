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

    discourse_indexer::domain::instances::repo::register_instance(
        &db,
        TEST_INSTANCE,
        "https://meta.discourse.org",
    )
    .await
    .expect("register instance");

    // Pre-fetch a large slice of directory users so we can satisfy FK for likers
    let dir_users =
        discourse_indexer::domain::users::adapter::fetch_users_directory(&client, true, 50, 50)
            .await
            .expect("fetch directory users for likes FK");
    let mut dir_by_id = std::collections::HashMap::new();
    for it in dir_users.iter() {
        dir_by_id.insert(it.user.id, it.clone());
    }

    // Search multiple pages of topics and posts until we find likes where all likers are in directory
    let mut found: Option<(
        discourse_indexer::domain::topics::model::Topic,
        discourse_indexer::domain::posts::model::Post,
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
                for p in posts.post_stream.posts.clone() {
                    match likes_adapter::fetch_post_likes(&client, p.id).await {
                        Ok(likes) if !likes.post_action_users.is_empty() => {
                            let filtered: Vec<_> = likes
                                .post_action_users
                                .into_iter()
                                .filter(|lu| dir_by_id.contains_key(&lu.id))
                                .collect();
                            if !filtered.is_empty() {
                                let likes_filtered =
                                    discourse_indexer::domain::likes::model::PostLikeResponse {
                                        post_action_users: filtered,
                                        total_rows_post_action_users: None,
                                    };
                                found = Some((t.clone(), p.clone(), likes_filtered));
                                break 'outer;
                            } else {
                                continue;
                            }
                        }
                        _ => continue,
                    }
                }
            }
        }
    }

    assert!(
        found.is_some(),
        "No post with likes found after scanning recent topics with directory coverage"
    );
    let (topic, post, likes) = found.unwrap();

    // Ensure categories exist for the topic's category_id (fetch first few pages)
    let cats = discourse_indexer::domain::categories::adapter::fetch_categories_all(&client, 2)
        .await
        .expect("fetch categories for topic FK");
    for c in cats.iter() {
        discourse_indexer::domain::categories::repo::upsert_category(&db, TEST_INSTANCE, c)
            .await
            .expect("upsert category for topic FK");
    }

    // Ensure topic and post are present to satisfy FKs before inserting likes
    discourse_indexer::domain::topics::repo::upsert_topics_bulk(
        &db,
        TEST_INSTANCE,
        &[topic.clone()],
    )
    .await
    .expect("upsert topic for likes");
    // Ensure minimal author user exists for post FK
    let minimal_user = discourse_indexer::domain::users::model::DirectoryItem {
        id: 0,
        likes_received: 0,
        likes_given: 0,
        topics_entered: 0,
        topic_count: 0,
        post_count: 0,
        posts_read: 0,
        days_visited: 0,
        user: discourse_indexer::domain::users::model::DirectoryUser {
            id: post.user_id,
            username: post.username.clone(),
            name: Some(
                post.display_username
                    .clone()
                    .unwrap_or_else(|| post.username.clone()),
            ),
            avatar_template: post.avatar_template.clone(),
            title: None,
        },
    };
    discourse_indexer::domain::users::repo::upsert_user(&db, TEST_INSTANCE, &minimal_user)
        .await
        .expect("upsert minimal user for post FK");

    discourse_indexer::domain::posts::repo::upsert_posts_bulk(&db, TEST_INSTANCE, &[post.clone()])
        .await
        .expect("upsert post for likes");

    // Upsert likers (guaranteed to be present in dir_by_id from the selection above)
    for u in likes.post_action_users.iter() {
        let item = dir_by_id
            .get(&u.id)
            .expect("liker present in directory map");
        discourse_indexer::domain::users::repo::upsert_user(&db, TEST_INSTANCE, item)
            .await
            .expect("upsert liker user");
    }

    // Persist and verify update path
    likes_repo::upsert_post_likes_batch(&db, TEST_INSTANCE, post.id, &likes)
        .await
        .expect("upsert likes");

    // Update: rename first user locally (from directory username) and re-upsert
    let mut likes_updated = likes.clone();
    if let Some(first) = likes_updated.post_action_users.get_mut(0) {
        first.username.push_str("_updated");
    }
    likes_repo::upsert_post_likes_batch(&db, TEST_INSTANCE, post.id, &likes_updated)
        .await
        .expect("update likes");

    let stored = discourse_post_likes::Entity::find()
        .filter(discourse_post_likes::Column::Instance.eq(TEST_INSTANCE))
        .filter(discourse_post_likes::Column::ExternalDiscoursePostId.eq(post.id))
        .filter(
            discourse_post_likes::Column::ExternalUserId.eq(likes_updated.post_action_users[0].id),
        )
        .one(&db)
        .await
        .expect("fetch like")
        .expect("like exists");
    assert!(stored.username.ends_with("_updated"));
}
