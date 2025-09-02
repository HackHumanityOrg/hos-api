// End-to-end integration tests for discourse indexing pipeline
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
use sea_orm::{EntityTrait, PaginatorTrait};

// Common testing utilities inlined for end-to-end tests
use migration::{sea_orm, MigratorTrait};
use sea_orm::DatabaseConnection;
use testcontainers::{runners::AsyncRunner, ImageExt};
use testcontainers_modules::mysql::Mysql;

/// Test database container setup for discourse indexer
pub struct TestDatabase {
    pub connection: DatabaseConnection,
    // Keep container alive for the duration of the test
    pub _container: testcontainers::ContainerAsync<testcontainers_modules::mysql::Mysql>,
}

impl TestDatabase {
    /// Create a new test database with MySQL container and discourse migrations
    pub async fn new() -> Self {
        // Start MySQL container
        let mysql_container = Mysql::default()
            .with_env_var("MYSQL_DATABASE", "discourse_test_db")
            .with_env_var("MYSQL_USER", "discourse_user")
            .with_env_var("MYSQL_PASSWORD", "discourse_password")
            .with_env_var("MYSQL_ROOT_PASSWORD", "root_password")
            .start()
            .await
            .expect("Failed to start MySQL container");

        let port = mysql_container
            .get_host_port_ipv4(3306)
            .await
            .expect("Failed to get MySQL port");

        // Create database URL
        let database_url = format!(
            "mysql://discourse_user:discourse_password@127.0.0.1:{}/discourse_test_db",
            port
        );

        // Create database connection with retry logic for container startup
        let connection = Self::connect_with_retry(&database_url, 10).await;

        // Run discourse migrations
        if let Err(e) = migration::Migrator::up(&connection, None).await {
            panic!("Failed to run discourse migrations: {}", e);
        }

        TestDatabase {
            connection,
            _container: mysql_container,
        }
    }

    /// Connect to database with retry logic
    async fn connect_with_retry(database_url: &str, max_attempts: u32) -> DatabaseConnection {
        for attempt in 1..=max_attempts {
            match sea_orm::Database::connect(database_url).await {
                Ok(conn) => return conn,
                Err(e) => {
                    if attempt == max_attempts {
                        panic!(
                            "Failed to connect to test database after {} attempts: {}",
                            max_attempts, e
                        );
                    }
                    println!(
                        "Database connection attempt {} failed, retrying...",
                        attempt
                    );
                    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
                }
            }
        }
        unreachable!()
    }

    /// Get a clone of the database connection for testing
    pub fn db(&self) -> DatabaseConnection {
        self.connection.clone()
    }
}

/// Test client setup for meta discourse
pub fn create_test_client() -> discourse_indexer::discourse_api::DiscourseClient {
    discourse_indexer::discourse_api::DiscourseClient::new("https://meta.discourse.org".to_string())
}

/// Initialize test tracing (run once per test suite)
pub fn init_test_tracing() {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter("debug")
            .with_test_writer()
            .init();
    });
}

const TEST_INSTANCE: &str = "meta.discourse.org";

#[fixture]
async fn test_db() -> TestDatabase {
    TestDatabase::new().await
}

/// End-to-end: Index categories, topics, users, posts, likes, and revisions into MySQL.
#[rstest]
#[tokio::test]
async fn test_e2e_full_indexing_pipeline_mysql(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let db = test_db.db();
    let client = create_test_client();

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

    // Test 4: Index posts for a topic
    println!("Testing post indexing...");
    let topic_id = topic_response.topic_list.topics[0].id;
    let post_response = posts::adapter::fetch_posts_page(&client, topic_id, 0)
        .await
        .expect("fetch posts");

    assert!(
        !post_response.post_stream.posts.is_empty(),
        "Should fetch posts"
    );

    // Store posts in database
    posts::repo::upsert_posts_bulk(&db, TEST_INSTANCE, &post_response.post_stream.posts)
        .await
        .expect("upsert posts");

    // Verify posts were stored
    let stored_posts = discourse_posts::Entity::find()
        .all(&db)
        .await
        .expect("fetch stored posts");

    assert!(!stored_posts.is_empty(), "Posts should be stored");
    println!("✅ Indexed {} posts", stored_posts.len());

    // Test 5: Index likes for posts
    println!("Testing likes indexing...");
    let post_id = post_response.post_stream.posts[0].id;
    let likes = likes::adapter::fetch_post_likes(&client, post_id)
        .await
        .expect("fetch likes");

    // Store likes in database (may be empty, which is fine)
    if !likes.post_action_users.is_empty() {
        likes::repo::upsert_post_likes_batch(&db, TEST_INSTANCE, post_id, &likes)
            .await
            .expect("upsert likes");
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

        for topic in topics.topic_list.topics.into_iter().take(10) {
            let posts = match posts::adapter::fetch_posts_page(&client, topic.id, 0).await {
                Ok(posts) => posts,
                Err(_) => continue,
            };

            for post in posts.post_stream.posts {
                if post.can_view_edit_history && post.version > 1 {
                    if let Ok(revision) =
                        revisions::adapter::fetch_post_revision(&client, post.id, 2).await
                    {
                        // Store revision in database
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

    // Fetch and store a topic
    let topic_response = topics::adapter::fetch_topics_recent_page(&client, 0)
        .await
        .expect("fetch topics");

    let topic = &topic_response.topic_list.topics[0];
    let topic_id = topic.id;

    // First insert
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

    // Index a complete data flow: topic -> posts -> users -> likes
    let topic_response = topics::adapter::fetch_topics_recent_page(&client, 0)
        .await
        .expect("fetch topics");

    let topic = &topic_response.topic_list.topics[0];

    // Store topic
    topics::repo::upsert_topics_bulk(&db, TEST_INSTANCE, &[topic.clone()])
        .await
        .expect("store topic");

    // Fetch and store posts for this topic
    let post_response = posts::adapter::fetch_posts_page(&client, topic.id, 0)
        .await
        .expect("fetch posts");

    assert!(!post_response.post_stream.posts.is_empty());

    let post = &post_response.post_stream.posts[0];

    // Store post
    posts::repo::upsert_posts_bulk(&db, TEST_INSTANCE, &[post.clone()])
        .await
        .expect("store post");

    // Fetch and store users from the posts
    let users = users::adapter::fetch_users_directory(&client, true, 1, 0)
        .await
        .expect("fetch users");

    if !users.is_empty() {
        users::repo::upsert_user(&db, TEST_INSTANCE, &users[0])
            .await
            .expect("store user");
    }

    // Fetch and store likes
    let likes = likes::adapter::fetch_post_likes(&client, post.id)
        .await
        .expect("fetch likes");

    if !likes.post_action_users.is_empty() {
        likes::repo::upsert_post_likes_batch(&db, TEST_INSTANCE, post.id, &likes)
            .await
            .expect("store likes");
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
                let posts_to_store: Vec<_> = posts.post_stream.posts.into_iter().take(2).collect();
                if !posts_to_store.is_empty() {
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
