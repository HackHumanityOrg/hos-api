// End-to-end integration tests for discourse indexing pipeline
use discourse_indexer::discourse_api::DiscourseClient;
use discourse_indexer::domain::categories;
use discourse_indexer::domain::likes;
use discourse_indexer::domain::posts;
use discourse_indexer::domain::revisions;
use discourse_indexer::domain::topics;
use discourse_indexer::domain::users;
use entities::{
    discourse_categories, discourse_post_likes, discourse_post_revisions, discourse_posts,
    discourse_topics, discourse_users,
};
use rstest::*;
use sea_orm::DatabaseConnection;
use sea_orm::{EntityTrait, PaginatorTrait};

use test_support::{init_test_tracing, TestDatabase};

/// Test client setup for meta discourse
pub fn create_test_client() -> discourse_indexer::discourse_api::DiscourseClient {
    discourse_indexer::discourse_api::DiscourseClient::new("https://meta.discourse.org".to_string())
}

/// Initialize test tracing (run once per test suite)
// tracing initialized via test_support::init_test_tracing

const TEST_INSTANCE: &str = "meta.discourse.org";

#[fixture]
async fn test_db() -> TestDatabase {
    TestDatabase::new().await
}

async fn ensure_user_exists(
    db: &DatabaseConnection,
    client: &DiscourseClient,
    username: &str,
) -> bool {
    match users::adapter::fetch_user_profile(client, username).await {
        Ok(resp) => users::repo::upsert_user_profile(db, TEST_INSTANCE, &resp.user)
            .await
            .is_ok(),
        Err(_) => false,
    }
}

/// End-to-end: Index categories, topics, users, posts, likes, and revisions into MySQL.
#[rstest]
#[tokio::test]
async fn test_e2e_full_indexing_pipeline_mysql(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let client = create_test_client();

    // Ensure instance record exists for FK relations
    discourse_indexer::domain::instances::repo::register_instance(
        &db,
        TEST_INSTANCE,
        "https://meta.discourse.org",
    )
    .await
    .expect("register instance");

    // Ensure categories exist for topic FK
    let categories = categories::adapter::fetch_categories_all(&client, 3)
        .await
        .expect("fetch categories for FK");
    for c in &categories {
        categories::repo::upsert_category(&db, TEST_INSTANCE, c)
            .await
            .expect("upsert category for FK");
    }

    // Test 1: Index categories
    println!("Testing category indexing...");
    let categories = categories::adapter::fetch_categories_all(&client, 3)
        .await
        .expect("fetch categories");

    assert!(!categories.is_empty(), "Should fetch at least one category");

    // Store categories in database
    for category in &categories {
        categories::repo::upsert_category(&db, TEST_INSTANCE, category)
            .await
            .expect("upsert category");
    }

    // Verify categories were stored
    let stored_categories = discourse_categories::Entity::find()
        .all(&db)
        .await
        .expect("fetch stored categories");

    assert!(!stored_categories.is_empty(), "Categories should be stored");
    println!("✅ Indexed {} categories", stored_categories.len());

    // Test 2: Index topics
    println!("Testing topic indexing...");
    let topic_response = topics::adapter::fetch_topics_recent_page(&client, 0)
        .await
        .expect("fetch topics");

    assert!(
        !topic_response.topic_list.topics.is_empty(),
        "Should fetch topics"
    );

    // Store topics in database
    topics::repo::upsert_topics_bulk(&db, TEST_INSTANCE, &topic_response.topic_list.topics)
        .await
        .expect("upsert topics");

    // Verify topics were stored
    let stored_topics = discourse_topics::Entity::find()
        .all(&db)
        .await
        .expect("fetch stored topics");

    assert!(!stored_topics.is_empty(), "Topics should be stored");
    println!("✅ Indexed {} topics", stored_topics.len());

    // Test 3: Index users from directory
    println!("Testing user indexing...");
    let users = users::adapter::fetch_users_directory(&client, true, 1, 0)
        .await
        .expect("fetch users");

    assert!(!users.is_empty(), "Should fetch users");

    // Store users in database
    for user_item in &users {
        users::repo::upsert_user(&db, TEST_INSTANCE, user_item)
            .await
            .expect("upsert user");
    }

    // Verify users were stored
    let stored_users = discourse_users::Entity::find()
        .all(&db)
        .await
        .expect("fetch stored users");

    assert!(!stored_users.is_empty(), "Users should be stored");
    println!("✅ Indexed {} users", stored_users.len());

    // Test 4: Index posts for the first topic that has resolvable authors
    println!("Testing post indexing...");
    let mut filtered_posts: Vec<posts::model::Post> = Vec::new();
    'find_posts: for t in &topic_response.topic_list.topics {
        match posts::adapter::fetch_posts_page(&client, t.id, 0).await {
            Ok(pr) => {
                for p in pr.post_stream.posts.into_iter() {
                    if ensure_user_exists(&db, &client, &p.username).await {
                        filtered_posts.push(p);
                    }
                }
                if !filtered_posts.is_empty() {
                    break 'find_posts;
                }
            }
            Err(_) => continue,
        }
    }

    assert!(
        !filtered_posts.is_empty(),
        "Should fetch posts with resolvable authors"
    );

    posts::repo::upsert_posts_bulk(&db, TEST_INSTANCE, &filtered_posts)
        .await
        .expect("upsert posts");

    // Verify posts were stored
    let stored_posts = discourse_posts::Entity::find()
        .all(&db)
        .await
        .expect("fetch stored posts");

    assert!(!stored_posts.is_empty(), "Posts should be stored");
    println!("✅ Indexed {} posts", stored_posts.len());

    // Test 5: Index likes for the first stored post (if any)
    println!("Testing likes indexing...");
    if let Some(first_post) = filtered_posts.get(0) {
        let post_id = first_post.id;
        let likes = likes::adapter::fetch_post_likes(&client, post_id)
            .await
            .expect("fetch likes");

        if !likes.post_action_users.is_empty() {
            // Ensure liker users exist; only insert likes if all can be resolved
            use std::collections::HashSet;
            let mut seen: HashSet<i32> = HashSet::new();
            let mut can_insert_likes = true;
            for u in &likes.post_action_users {
                if seen.insert(u.id) {
                    if !ensure_user_exists(&db, &client, &u.username).await {
                        can_insert_likes = false;
                        break;
                    }
                }
            }
            if can_insert_likes {
                likes::repo::upsert_post_likes_batch(&db, TEST_INSTANCE, post_id, &likes)
                    .await
                    .expect("upsert likes");
            }
        }
    }

    // Verify likes table exists and can be queried
    let stored_likes = discourse_post_likes::Entity::find()
        .all(&db)
        .await
        .expect("fetch stored likes");

    println!("✅ Indexed {} likes", stored_likes.len());

    // Test 6: Search for and index revisions
    println!("Testing revision indexing...");
    let mut revision_count = 0;

    // Search through multiple topics and pages to find revisions
    'search: for page in 0..3 {
        let topics = match topics::adapter::fetch_topics_recent_page(&client, page).await {
            Ok(topics) => topics,
            Err(_) => continue,
        };

        for topic in topics.topic_list.topics.clone().into_iter().take(10) {
            let posts = match posts::adapter::fetch_posts_page(&client, topic.id, 0).await {
                Ok(posts) => posts,
                Err(_) => continue,
            };

            for post in posts.post_stream.posts.clone() {
                if post.can_view_edit_history && post.version > 1 {
                    if let Ok(revision) =
                        revisions::adapter::fetch_post_revision(&client, post.id, 2).await
                    {
                        // Ensure FK chain exists for revisions -> posts (and posts -> topic/user)
                        // 1) Upsert the topic (requires categories already present)
                        let _ =
                            topics::repo::upsert_topics_bulk(&db, TEST_INSTANCE, &[topic.clone()])
                                .await;

                        // 2) Ensure the post author exists
                        let _ = ensure_user_exists(&db, &client, &post.username).await;

                        // 3) Upsert the post for FK target (topic/user must exist)
                        let _ = posts::repo::upsert_posts_bulk(&db, TEST_INSTANCE, &[post.clone()])
                            .await;

                        // 4) Store revision in database (FK to (instance, external_post_id))
                        revisions::repo::upsert_revision(&db, TEST_INSTANCE, post.id, &revision)
                            .await
                            .expect("upsert revision");

                        revision_count += 1;

                        // Found at least one revision, that's enough for testing
                        if revision_count >= 1 {
                            break 'search;
                        }
                    }
                }
            }
        }
    }

    // Verify revisions were stored (if any were found)
    let stored_revisions = discourse_post_revisions::Entity::find()
        .all(&db)
        .await
        .expect("fetch stored revisions");

    println!("✅ Indexed {} revisions", stored_revisions.len());

    // Final verification: Ensure all data types are correctly stored
    println!("\n📊 Final indexing summary:");
    println!("Categories: {} stored", stored_categories.len());
    println!("Topics: {} stored", stored_topics.len());
    println!("Users: {} stored", stored_users.len());
    println!("Posts: {} stored", stored_posts.len());
    println!("Likes: {} stored", stored_likes.len());
    println!("Revisions: {} stored", stored_revisions.len());

    // Ensure we have indexed core data
    assert!(
        stored_categories.len() > 0,
        "Should have indexed categories"
    );
    assert!(stored_topics.len() > 0, "Should have indexed topics");
    assert!(stored_users.len() > 0, "Should have indexed users");
    assert!(stored_posts.len() > 0, "Should have indexed posts");

    println!("✅ Full discourse indexing pipeline test completed successfully!");
}

/// End-to-end: Upsert semantics on topics (no duplicates, updates in place).
#[rstest]
#[tokio::test]
async fn test_e2e_database_persistence_and_upsert(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let client = create_test_client();

    // Ensure instance record exists for FK relations
    discourse_indexer::domain::instances::repo::register_instance(
        &db,
        TEST_INSTANCE,
        "https://meta.discourse.org",
    )
    .await
    .expect("register instance");

    // Ensure categories exist for topic FK
    let categories = categories::adapter::fetch_categories_all(&client, 3)
        .await
        .expect("fetch categories for FK");
    for c in &categories {
        categories::repo::upsert_category(&db, TEST_INSTANCE, c)
            .await
            .expect("upsert category for FK");
    }

    // Fetch and store a topic
    let topic_response = topics::adapter::fetch_topics_recent_page(&client, 0)
        .await
        .expect("fetch topics");

    let topic = &topic_response.topic_list.topics[0];
    let topic_id = topic.id;

    // First insert (after categories available)
    topics::repo::upsert_topics_bulk(&db, TEST_INSTANCE, &[topic.clone()])
        .await
        .expect("first upsert");

    let topics_count = discourse_topics::Entity::find()
        .count(&db)
        .await
        .expect("count topics");

    assert_eq!(topics_count, 1, "Should have one topic after first insert");

    // Second insert (should update, not duplicate)
    topics::repo::upsert_topics_bulk(&db, TEST_INSTANCE, &[topic.clone()])
        .await
        .expect("second upsert");

    let topics_count_after = discourse_topics::Entity::find()
        .count(&db)
        .await
        .expect("count topics after second insert");

    assert_eq!(
        topics_count_after, 1,
        "Should still have one topic after upsert"
    );

    // Verify the topic data is correct
    let stored_topic = discourse_topics::Entity::find()
        .one(&db)
        .await
        .expect("fetch topic")
        .expect("topic should exist");

    assert_eq!(stored_topic.instance, TEST_INSTANCE);
    assert_eq!(stored_topic.external_id, topic_id);
    assert_eq!(stored_topic.title, topic.title);
    assert_eq!(stored_topic.slug, topic.slug);

    println!("✅ Database persistence and upsert functionality verified");
}

/// End-to-end: Integrity across related entities (topic, post, users, likes).
#[rstest]
#[tokio::test]
async fn test_e2e_data_integrity_across_entities(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let client = create_test_client();

    // Ensure instance record exists for FK relations
    discourse_indexer::domain::instances::repo::register_instance(
        &db,
        TEST_INSTANCE,
        "https://meta.discourse.org",
    )
    .await
    .expect("register instance");

    // Index a complete data flow: topic -> posts -> users -> likes
    let topic_response = topics::adapter::fetch_topics_recent_page(&client, 0)
        .await
        .expect("fetch topics");

    let topic = &topic_response.topic_list.topics[0];

    // Ensure categories exist for topic FK
    let categories = categories::adapter::fetch_categories_all(&client, 3)
        .await
        .expect("fetch categories for FK");
    for c in &categories {
        categories::repo::upsert_category(&db, TEST_INSTANCE, c)
            .await
            .expect("upsert category for FK");
    }

    // Store topic (after categories available)
    topics::repo::upsert_topics_bulk(&db, TEST_INSTANCE, &[topic.clone()])
        .await
        .expect("store topic");

    // Fetch and store posts for this topic
    let post_response = posts::adapter::fetch_posts_page(&client, topic.id, 0)
        .await
        .expect("fetch posts");

    assert!(!post_response.post_stream.posts.is_empty());

    // Ensure post author exists as user (FK), fetched from API; if not available, pick another post
    let mut chosen_post = None;
    for p in post_response.post_stream.posts.iter() {
        if ensure_user_exists(&db, &client, &p.username).await {
            chosen_post = Some(p.clone());
            break;
        }
    }
    let post = chosen_post.expect("find at least one post with resolvable author");

    // Store post
    posts::repo::upsert_posts_bulk(&db, TEST_INSTANCE, &[post.clone()])
        .await
        .expect("store post");

    // Optionally fetch directory users to populate more info (not required for FK)

    // Fetch and store likes
    let likes = likes::adapter::fetch_post_likes(&client, post.id)
        .await
        .expect("fetch likes");

    if !likes.post_action_users.is_empty() {
        // Ensure liker users exist; only insert likes if all can be resolved
        let mut seen = std::collections::HashSet::new();
        let mut can_insert_likes = true;
        for u in &likes.post_action_users {
            if seen.insert(u.id) {
                if !ensure_user_exists(&db, &client, &u.username).await {
                    can_insert_likes = false;
                    break;
                }
            }
        }
        if can_insert_likes {
            likes::repo::upsert_post_likes_batch(&db, TEST_INSTANCE, post.id, &likes)
                .await
                .expect("store likes");
        }
    }

    // Verify data relationships
    let stored_topics = discourse_topics::Entity::find()
        .all(&db)
        .await
        .expect("fetch topics");

    let stored_posts = discourse_posts::Entity::find()
        .all(&db)
        .await
        .expect("fetch posts");

    let stored_users = discourse_users::Entity::find()
        .all(&db)
        .await
        .expect("fetch users");

    let stored_likes = discourse_post_likes::Entity::find()
        .all(&db)
        .await
        .expect("fetch likes");

    // Verify referential integrity
    assert!(!stored_topics.is_empty(), "Should have stored topics");
    assert!(!stored_posts.is_empty(), "Should have stored posts");

    let stored_topic = &stored_topics[0];
    let stored_post = &stored_posts[0];

    // Verify post references correct topic
    assert_eq!(
        stored_post.topic_id, stored_topic.external_id,
        "Post should reference correct topic"
    );
    assert_eq!(
        stored_post.instance, stored_topic.instance,
        "Post and topic should have same instance"
    );

    // Verify likes reference correct post (if any likes exist)
    if !stored_likes.is_empty() {
        let stored_like = &stored_likes[0];
        assert_eq!(
            stored_like.external_discourse_post_id, stored_post.external_id,
            "Like should reference correct post"
        );
        assert_eq!(
            stored_like.instance, stored_post.instance,
            "Like and post should have same instance"
        );
    }

    println!("✅ Data integrity verified across related entities");
    println!(
        "Topics: {}, Posts: {}, Users: {}, Likes: {}",
        stored_topics.len(),
        stored_posts.len(),
        stored_users.len(),
        stored_likes.len()
    );
}

/// End-to-end: API error handling and DB recovery remain functional.
#[rstest]
#[tokio::test]
async fn test_e2e_error_handling_and_recovery(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let client = create_test_client();

    // Ensure instance record exists for FK relations
    discourse_indexer::domain::instances::repo::register_instance(
        &db,
        TEST_INSTANCE,
        "https://meta.discourse.org",
    )
    .await
    .expect("register instance");

    // Ensure categories exist for topic FK (for later topic upsert)
    let categories = categories::adapter::fetch_categories_all(&client, 3)
        .await
        .expect("fetch categories for FK");
    for c in &categories {
        categories::repo::upsert_category(&db, TEST_INSTANCE, c)
            .await
            .expect("upsert category for FK");
    }

    // Test with invalid topic ID (should handle gracefully)
    let invalid_topic_id = 999999999;
    let result = posts::adapter::fetch_posts_page(&client, invalid_topic_id, 0).await;

    match result {
        Ok(_) => println!("Invalid topic ID unexpectedly returned data"),
        Err(e) => println!("✅ Handled invalid topic ID gracefully: {}", e),
    }

    // Test with invalid post ID for likes
    let invalid_post_id = 999999999;
    let likes_result = likes::adapter::fetch_post_likes(&client, invalid_post_id).await;

    match likes_result {
        Ok(likes) => {
            // Empty likes response is acceptable
            println!(
                "✅ Invalid post ID returned {} likes (acceptable)",
                likes.post_action_users.len()
            );
        }
        Err(e) => println!("✅ Handled invalid post ID gracefully: {}", e),
    }

    // Test database operations still work after errors
    let topic_response = topics::adapter::fetch_topics_recent_page(&client, 0)
        .await
        .expect("fetch topics should still work");

    assert!(!topic_response.topic_list.topics.is_empty());

    let topic = &topic_response.topic_list.topics[0];
    topics::repo::upsert_topics_bulk(&db, TEST_INSTANCE, &[topic.clone()])
        .await
        .expect("database operations should still work after API errors");

    let stored_topics = discourse_topics::Entity::find()
        .count(&db)
        .await
        .expect("count topics");

    assert_eq!(
        stored_topics, 1,
        "Should have stored one topic despite earlier errors"
    );

    println!("✅ Error handling and recovery test completed");
}

/// End-to-end: Concurrent indexing of multiple topics persists without corruption.
#[rstest]
#[tokio::test]
async fn test_e2e_concurrent_indexing_across_topics(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let client = create_test_client();

    // Ensure instance record exists for FK relations
    discourse_indexer::domain::instances::repo::register_instance(
        &db,
        TEST_INSTANCE,
        "https://meta.discourse.org",
    )
    .await
    .expect("register instance");

    // Ensure categories exist for topic FK
    let categories = categories::adapter::fetch_categories_all(&client, 3)
        .await
        .expect("fetch categories for FK");
    for c in &categories {
        categories::repo::upsert_category(&db, TEST_INSTANCE, c)
            .await
            .expect("upsert category for FK");
    }

    // Ensure instance record exists for FK relations
    discourse_indexer::domain::instances::repo::register_instance(
        &db,
        TEST_INSTANCE,
        "https://meta.discourse.org",
    )
    .await
    .expect("register instance");

    // Fetch topics to work with
    let topic_response = topics::adapter::fetch_topics_recent_page(&client, 0)
        .await
        .expect("fetch topics");

    let topics_to_process = topic_response
        .topic_list
        .topics
        .into_iter()
        .take(3)
        .collect::<Vec<_>>();

    // Process multiple topics concurrently
    let mut handles = Vec::new();

    for (i, topic) in topics_to_process.iter().enumerate() {
        let db_clone = db.clone();
        let client_clone = client.clone();
        let topic_clone = topic.clone();

        let handle = tokio::spawn(async move {
            println!("Processing topic {} concurrently: {}", i, topic_clone.title);

            // Store topic
            topics::repo::upsert_topics_bulk(&db_clone, TEST_INSTANCE, &[topic_clone.clone()])
                .await
                .expect("concurrent topic upsert");

            // Fetch and store posts
            if let Ok(posts) =
                posts::adapter::fetch_posts_page(&client_clone, topic_clone.id, 0).await
            {
                let mut posts_to_store: Vec<_> = Vec::new();
                for p in posts.post_stream.posts.into_iter().take(2) {
                    if ensure_user_exists(&db_clone, &client_clone, &p.username).await {
                        posts_to_store.push(p);
                    }
                }
                if !posts_to_store.is_empty() {
                    // Seed users for these posts to satisfy FK using real data
                    posts::repo::upsert_posts_bulk(&db_clone, TEST_INSTANCE, &posts_to_store)
                        .await
                        .expect("concurrent posts upsert");
                }
            }

            i // Return index for verification
        });

        handles.push(handle);
    }

    // Wait for all concurrent operations to complete
    let results = futures::future::join_all(handles).await;

    // Verify all operations completed successfully
    for (expected_index, result) in results.into_iter().enumerate() {
        let actual_index = result.expect("concurrent operation should complete successfully");
        assert_eq!(
            actual_index, expected_index,
            "Concurrent operations should complete in order"
        );
    }

    // Verify data integrity after concurrent operations
    let stored_topics = discourse_topics::Entity::find()
        .count(&db)
        .await
        .expect("count topics");

    let stored_posts = discourse_posts::Entity::find()
        .count(&db)
        .await
        .expect("count posts");

    assert_eq!(stored_topics, 3, "Should have stored all 3 topics");
    assert!(stored_posts > 0, "Should have stored posts");

    println!("✅ Concurrent indexing operations completed successfully");
    println!(
        "Stored {} topics and {} posts concurrently",
        stored_topics, stored_posts
    );
}
