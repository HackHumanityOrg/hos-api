use anyhow::{anyhow, Result};
use async_trait::async_trait;
use discourse_indexer::discourse_api::JsonFetcher;
use discourse_indexer::domain::{categories, likes, posts, revisions, topics, users};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

#[path = "support/mod.rs"]
mod support;

use support::FixtureFetcher;

struct MapFetcher {
    responses: HashMap<String, Value>,
}

impl MapFetcher {
    fn new(mapping: &[(&str, Value)]) -> Self {
        let mut responses = HashMap::new();
        for (path, payload) in mapping {
            responses.insert((*path).to_string(), payload.clone());
        }
        Self { responses }
    }
}

#[async_trait]
impl JsonFetcher for MapFetcher {
    async fn get_json<T: DeserializeOwned + Send + 'static>(&self, path: &str) -> Result<T> {
        let value = self
            .responses
            .get(path)
            .ok_or_else(|| anyhow!("missing response for path {path}"))?;
        Ok(serde_json::from_value(value.clone())?)
    }
}

#[tokio::test]
async fn categories_stop_when_root_ids_repeat() -> Result<()> {
    let single_page = FixtureFetcher::new(&[(
        "/categories.json?include_subcategories=true&page=0",
        "meta/categories_page_0.json",
    )])?;
    let expected = categories::adapter::fetch_categories_all(&single_page, 0).await?;

    let repeated = FixtureFetcher::new(&[
        (
            "/categories.json?include_subcategories=true&page=0",
            "meta/categories_page_0.json",
        ),
        (
            "/categories.json?include_subcategories=true&page=1",
            "meta/categories_page_0.json",
        ),
    ])?;
    let actual = categories::adapter::fetch_categories_all(&repeated, 10).await?;

    assert_eq!(actual.len(), expected.len());
    Ok(())
}

#[tokio::test]
async fn users_directory_deduplicates_across_pages() -> Result<()> {
    let page0 = json!({
        "directory_items": [directory_item(1, "alice"), directory_item(2, "bob")],
        "meta": directory_meta(3),
    });
    let page1 = json!({
        "directory_items": [directory_item(2, "bob"), directory_item(3, "carol")],
        "meta": directory_meta(3),
    });

    let fetcher = MapFetcher::new(&[
        (
            "/directory_items.json?period=all&order=username&page=0&asc=true",
            page0,
        ),
        (
            "/directory_items.json?period=all&order=username&page=1&asc=true",
            page1,
        ),
    ]);

    let users = users::adapter::fetch_users_directory(&fetcher, false, 1, 10).await?;
    let ids: HashSet<i32> = users.iter().map(|u| u.user.id).collect();

    assert_eq!(users.len(), 3);
    assert_eq!(ids.len(), 3);
    Ok(())
}

#[tokio::test]
async fn likes_adapter_deduplicates_and_honors_total_rows() -> Result<()> {
    let page0 = json!({
        "post_action_users": [
            like_user(10, "alice"),
            like_user(11, "bob")
        ],
        "total_rows_post_action_users": 3
    });
    let page1 = json!({
        "post_action_users": [
            like_user(11, "bob"),
            like_user(12, "carol")
        ],
        "total_rows_post_action_users": 3
    });

    let fetcher = MapFetcher::new(&[
        (
            "/post_action_users.json?id=77&post_action_type_id=2&page=0&limit=50",
            page0,
        ),
        (
            "/post_action_users.json?id=77&post_action_type_id=2&page=1&limit=50",
            page1,
        ),
    ]);

    let likes = likes::adapter::fetch_post_likes(&fetcher, 77).await?;
    let ids: HashSet<i32> = likes.iter().map(|u| u.id).collect();

    assert_eq!(likes.len(), 3);
    assert_eq!(ids.len(), 3);
    Ok(())
}

#[tokio::test]
async fn posts_adapter_uses_one_based_pagination() -> Result<()> {
    let fetcher = FixtureFetcher::new(&[(
        "/t/69776.json?page=1&include_raw=true",
        "meta/topic_69776_posts_page_0.json",
    )])?;

    let response = posts::adapter::fetch_posts_page(&fetcher, 69776, 0).await?;
    assert!(!response.post_stream.posts.is_empty());
    assert_eq!(response.post_stream.posts[0].topic_id, 69776);

    Ok(())
}

#[tokio::test]
async fn topics_created_adapter_builds_created_sort_path() -> Result<()> {
    let fetcher = MapFetcher::new(&[(
        "/latest.json?order=created&ascending=true&page=2",
        json!({
            "topic_list": {
                "topics": [minimal_topic(9001, 123)],
                "per_page": 1
            }
        }),
    )]);

    let response = topics::adapter::fetch_topics_created_page(&fetcher, 2).await?;
    assert_eq!(response.topic_list.topics.len(), 1);
    assert_eq!(response.topic_list.topics[0].id, 9001);

    Ok(())
}

#[tokio::test]
async fn revisions_adapter_builds_revision_path() -> Result<()> {
    let fetcher = FixtureFetcher::new(&[(
        "/posts/326680/revisions/2.json",
        "meta/post_326680_revision_2.json",
    )])?;

    let revision = revisions::adapter::fetch_post_revision(&fetcher, 326680, 2).await?;
    assert_eq!(revision.post_id, 326680);
    assert_eq!(revision.current_version, 2);

    Ok(())
}

fn directory_meta(total_rows: u64) -> Value {
    json!({
        "last_updated_at": "2026-01-01T00:00:00.000Z",
        "total_rows_directory_items": total_rows,
        "load_more_directory_items": null
    })
}

fn directory_item(id: i32, username: &str) -> Value {
    json!({
        "id": id,
        "likes_received": 0,
        "likes_given": 0,
        "topics_entered": 0,
        "topic_count": 0,
        "post_count": 0,
        "posts_read": 0,
        "days_visited": 0,
        "user": {
            "id": id,
            "username": username,
            "name": username,
            "avatar_template": "/user_avatar/example/{size}/1_2.png",
            "title": null,
            "trust_level": 2
        }
    })
}

fn like_user(id: i32, username: &str) -> Value {
    json!({
        "id": id,
        "username": username,
        "name": username,
        "avatar_template": "/user_avatar/example/{size}/1_2.png",
        "post_url": null,
        "username_lower": username
    })
}

fn minimal_topic(id: i32, category_id: i32) -> Value {
    json!({
        "id": id,
        "title": format!("Topic {id}"),
        "fancy_title": format!("Topic {id}"),
        "slug": format!("topic-{id}"),
        "posts_count": 1,
        "reply_count": 0,
        "highest_post_number": 1,
        "image_url": null,
        "created_at": "2026-01-01T00:00:00Z",
        "last_posted_at": "2026-01-01T00:00:00Z",
        "bumped": true,
        "bumped_at": "2026-01-01T00:00:00Z",
        "archetype": "regular",
        "pinned": false,
        "visible": true,
        "closed": false,
        "archived": false,
        "views": 10,
        "like_count": 0,
        "has_summary": false,
        "last_poster_username": null,
        "category_id": category_id,
        "pinned_globally": false
    })
}
