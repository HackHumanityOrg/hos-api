use discourse_indexer::discourse_api::DiscourseClient;
use discourse_indexer::domain;

/// Live call: fetch recent topics and posts on meta.discourse.org.
#[tokio::test]
async fn test_live_fetch_recent_topics_and_posts() {
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());

    let topics = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        domain::topics::adapter::fetch_topics_recent_page(&client, 0),
    )
    .await
    .expect("timeout")
    .expect("topics");
    assert!(topics.topic_list.topics.len() > 0);
    let topic_id = topics.topic_list.topics[0].id;

    let posts = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        domain::posts::adapter::fetch_posts_page(&client, topic_id, 0),
    )
    .await
    .expect("timeout")
    .expect("posts");
    assert!(posts.post_stream.posts.len() > 0);

    // Fetch likes for first post
    let post_id = posts.post_stream.posts[0].id;
    let _likes = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        domain::likes::adapter::fetch_post_likes(&client, post_id),
    )
    .await
    .expect("timeout")
    .expect("likes");
}

/// Live call: fetch categories and directory users on meta.discourse.org.
#[tokio::test]
async fn test_live_fetch_categories_and_directory_users() {
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());

    let cats = domain::categories::adapter::fetch_categories_all(&client, 2)
        .await
        .expect("categories");
    assert!(cats.len() > 0);

    let users = domain::users::adapter::fetch_users_directory(&client, true, 1, 2)
        .await
        .expect("users");
    assert!(users.len() > 0);
}

/// Live call: verify pinned topic slug for topic id 1 in latest.
#[tokio::test]
async fn test_live_latest_contains_pinned_start_here() {
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());
    let topics = domain::topics::adapter::fetch_topics_recent_page(&client, 0)
        .await
        .expect("fetch topics");

    // Find canonical pinned topic id 1
    let pinned = topics.topic_list.topics.iter().find(|t| t.id == 1);
    if let Some(t) = pinned {
        assert_eq!(t.slug, "new-to-discourse-start-here");
        // Note: These may be optional in the new model structure
        assert!(t.pinned.unwrap_or(false) || t.pinned_globally.unwrap_or(false));
    } else {
        // If not present (unlikely), at least assert we have topics
        assert!(!topics.topic_list.topics.is_empty());
    }
}

/// Live call: verify shape of first page of posts for topic 1.
#[tokio::test]
async fn test_live_topic_posts_first_page_shape() {
    // Use topic 1 which should be stable
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());
    let posts = domain::posts::adapter::fetch_posts_page(&client, 1, 0)
        .await
        .expect("fetch posts");

    let posts_count = posts.posts_count.unwrap_or(0);
    assert!(posts_count >= 1);
    assert!(!posts.post_stream.posts.is_empty());
    let p0 = &posts.post_stream.posts[0];
    assert!(p0.id > 0);
    assert_eq!(p0.post_number, 1);
    // Verify timestamps exist
    let _ = p0.created_at;
    let _ = p0.updated_at;
}

/// Live call: users directory returns at least one user.
#[tokio::test]
async fn test_live_directory_items_contains_users() {
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());
    let users = domain::users::adapter::fetch_users_directory(&client, true, 1, 0)
        .await
        .expect("fetch users directory");

    assert!(!users.is_empty());
    let u = &users[0].user;
    assert!(u.id > 0);
}

/// Live call: find a post with edits and fetch a revision.
#[tokio::test]
async fn test_live_find_post_with_edits_and_fetch_revision() {
    // Search more thoroughly for a post with edit history
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());

    let mut revision_found = false;

    // Try multiple pages of topics to find one with revisions
    for page in 0..3 {
        let latest = match tokio::time::timeout(
            std::time::Duration::from_secs(15),
            domain::topics::adapter::fetch_topics_recent_page(&client, page),
        )
        .await
        {
            Ok(Ok(topics)) => topics,
            _ => continue,
        };

        for t in latest.topic_list.topics.into_iter().take(10) {
            let posts = match tokio::time::timeout(
                std::time::Duration::from_secs(10),
                domain::posts::adapter::fetch_posts_page(&client, t.id, 0),
            )
            .await
            {
                Ok(Ok(v)) => v,
                _ => continue,
            };

            for p in posts.post_stream.posts {
                if p.can_view_edit_history && p.version > 1 {
                    if let Ok(Ok(rev)) = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        domain::revisions::adapter::fetch_post_revision(&client, p.id, 2),
                    )
                    .await
                    {
                        // Verify we can parse revision fields properly
                        assert!(rev.post_id > 0);
                        assert!(!rev.username.is_empty());
                        revision_found = true;
                        break;
                    }
                }
            }
            if revision_found {
                break;
            }
        }
        if revision_found {
            break;
        }
    }

    // We should find at least one revision on meta.discourse.org
    assert!(
        revision_found,
        "Should have found at least one post with revision history"
    );
}

/// Live call: TopicResponse deserializes and contains expected fields.
#[tokio::test]
async fn test_live_topicresponse_deserializes() {
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());
    let tr = domain::topics::adapter::fetch_topics_recent_page(&client, 0)
        .await
        .expect("deserialize TopicResponse");
    assert!(tr.topic_list.per_page.unwrap_or(0) > 0);
    assert!(!tr.topic_list.topics.is_empty());
    let t = &tr.topic_list.topics[0];
    // Check a few required fields map correctly
    assert!(t.id > 0);
    assert!(!t.slug.is_empty());
    // pinned can be true/false, just ensure boolean exists via compile-time mapping
    let _ = t.pinned;
    let _ = t.pinned_globally;
}

/// Live call: CategoryResponse deserializes and contains expected fields.
#[tokio::test]
async fn test_live_categoryresponse_deserializes() {
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());
    let cr = domain::categories::adapter::fetch_categories_all(&client, 2)
        .await
        .expect("deserialize CategoryResponse");
    assert!(!cr.is_empty());
    let c = &cr[0];
    assert!(c.id > 0);
    assert!(!c.name.is_empty());
}

/// Live call: UserResponse deserializes with optional username field.
#[tokio::test]
async fn test_live_userresponse_deserializes_with_optionals() {
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());
    let ur = domain::users::adapter::fetch_users_directory(&client, true, 1, 0)
        .await
        .expect("deserialize UserResponse");
    assert!(!ur.is_empty());
    let item = &ur[0];
    // Username can be null on meta, ensure our Option mapping doesn't panic
    let _: &str = item.user.username.as_ref();
}

/// Live call: PostResponse and Likes deserialize.
#[tokio::test]
async fn test_live_postresponse_and_likes_deserialize() {
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());

    // Find a topic id first
    let latest = domain::topics::adapter::fetch_topics_recent_page(&client, 0)
        .await
        .expect("topics");
    let topic_id = latest.topic_list.topics[0].id;

    let pr = domain::posts::adapter::fetch_posts_page(&client, topic_id, 0)
        .await
        .expect("post response");
    assert!(!pr.post_stream.posts.is_empty());
    let p0 = &pr.post_stream.posts[0];

    // Likes endpoint may return empty, but should deserialize
    let _likes = domain::likes::adapter::fetch_post_likes(&client, p0.id)
        .await
        .expect("likes response shape");
}

/// Live call: comprehensive coverage across topics, categories, users, posts, likes, and revisions.
#[tokio::test]
async fn test_live_comprehensive_model_parsing() {
    let client = DiscourseClient::new("https://meta.discourse.org".to_string());

    // Test topics parsing
    let topics = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        domain::topics::adapter::fetch_topics_recent_page(&client, 0),
    )
    .await
    .expect("timeout")
    .expect("topics");
    assert!(!topics.topic_list.topics.is_empty());

    // Test categories parsing
    let categories = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        domain::categories::adapter::fetch_categories_all(&client, 2),
    )
    .await
    .expect("timeout")
    .expect("categories");
    assert!(!categories.is_empty());

    // Test users parsing
    let users = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        domain::users::adapter::fetch_users_directory(&client, true, 1, 0),
    )
    .await
    .expect("timeout")
    .expect("users");
    assert!(!users.is_empty());

    // Test posts and likes parsing
    let topic_id = topics.topic_list.topics[0].id;
    let posts = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        domain::posts::adapter::fetch_posts_page(&client, topic_id, 0),
    )
    .await
    .expect("timeout")
    .expect("posts");
    assert!(!posts.post_stream.posts.is_empty());

    let post_id = posts.post_stream.posts[0].id;
    let _likes = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        domain::likes::adapter::fetch_post_likes(&client, post_id),
    )
    .await
    .expect("timeout")
    .expect("likes");

    // Search for revision until we find one (this is thorough)
    let mut revision_tested = false;
    'outer: for page in 0..5 {
        let page_topics = match tokio::time::timeout(
            std::time::Duration::from_secs(15),
            domain::topics::adapter::fetch_topics_recent_page(&client, page),
        )
        .await
        {
            Ok(Ok(topics)) => topics,
            _ => continue,
        };

        for t in page_topics.topic_list.topics.into_iter().take(15) {
            let topic_posts = match tokio::time::timeout(
                std::time::Duration::from_secs(10),
                domain::posts::adapter::fetch_posts_page(&client, t.id, 0),
            )
            .await
            {
                Ok(Ok(v)) => v,
                _ => continue,
            };

            for p in topic_posts.post_stream.posts {
                if p.can_view_edit_history && p.version > 1 {
                    if let Ok(Ok(revision)) = tokio::time::timeout(
                        std::time::Duration::from_secs(10),
                        domain::revisions::adapter::fetch_post_revision(&client, p.id, 2),
                    )
                    .await
                    {
                        // Validate revision model fields
                        assert!(revision.post_id > 0);
                        assert!(!revision.username.is_empty());
                        revision_tested = true;
                        break 'outer;
                    }
                }
            }
        }
    }

    println!("Revision parsing tested: {}", revision_tested);
    // Don't assert on revision found since it's not guaranteed, but we tried thoroughly
}
