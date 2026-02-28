use crate::discourse_api::JsonFetcher;
use crate::domain::users::model::{DirectoryItem, User, UserProfileResponse, UserResponse};
use anyhow::{Context, Result};
use sea_orm::DatabaseConnection;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{debug, instrument, warn};

#[derive(Clone, Default)]
pub struct KnownUsersCache {
    usernames: Arc<Mutex<HashSet<String>>>,
}

impl KnownUsersCache {
    pub fn new() -> Self {
        Self::default()
    }

    fn normalize(username: &str) -> String {
        username.to_ascii_lowercase()
    }

    pub async fn try_mark_seen(&self, username: &str) -> bool {
        let mut usernames = self.usernames.lock().await;
        usernames.insert(Self::normalize(username))
    }

    pub async fn unmark(&self, username: &str) {
        let mut usernames = self.usernames.lock().await;
        usernames.remove(&Self::normalize(username));
    }
}

#[instrument(skip(client), fields(recent = recent, page_limit = page_limit, max_pages = max_pages))]
pub async fn fetch_users_directory<F: JsonFetcher>(
    client: &F,
    recent: bool,
    page_limit: u32,
    max_pages: u32,
) -> Result<Vec<DirectoryItem>> {
    let mut all_users = Vec::new();
    let mut seen_user_ids = HashSet::new();
    let mut prev_page_ids: Option<Vec<i32>> = None;
    let mut page = 0;

    loop {
        if page >= max_pages {
            break;
        }

        let (order, period, asc) = if recent {
            ("post_count", "weekly", false)
        } else {
            ("username", "all", true)
        };
        let mut path = format!(
            "/directory_items.json?period={}&order={}&page={}",
            period, order, page
        );
        if asc {
            path.push_str("&asc=true");
        }

        let response: UserResponse = client.get_json(&path).await?;
        debug!(
            page,
            count = response.directory_items.len(),
            recent,
            "fetched users directory page"
        );

        if response.directory_items.is_empty() {
            break;
        }

        let page_ids: Vec<i32> = response
            .directory_items
            .iter()
            .map(|item| item.user.id)
            .collect();
        if let Some(prev) = &prev_page_ids {
            if prev == &page_ids {
                debug!(page, "detected repeated users page, stopping");
                break;
            }
        }
        prev_page_ids = Some(page_ids);

        for item in response.directory_items {
            if seen_user_ids.insert(item.user.id) {
                all_users.push(item);
            }
        }

        if page >= page_limit {
            break;
        }

        page += 1;
    }

    debug!(total = all_users.len(), "fetched users directory total");
    Ok(all_users)
}

#[instrument(skip(client), fields(username = username))]
pub async fn fetch_directory_user_by_username<F: JsonFetcher>(
    client: &F,
    username: &str,
) -> Result<Option<DirectoryItem>> {
    let url = reqwest::Url::parse_with_params(
        "https://example.invalid/directory_items.json",
        [
            ("period", "all"),
            ("order", "username"),
            ("asc", "true"),
            ("username", username),
        ],
    )
    .context("failed to build directory username URL")?;

    let query = url.query().unwrap_or_default();
    let path = format!("{}?{}", url.path(), query);
    let mut response: UserResponse = client.get_json(&path).await?;

    if response.directory_items.is_empty() {
        debug!(username = username, "no directory item for username");
        return Ok(None);
    }

    let Some(idx) = response
        .directory_items
        .iter()
        .position(|item| item.user.username.eq_ignore_ascii_case(username))
    else {
        debug!(
            username = username,
            count = response.directory_items.len(),
            "directory username filter returned items but no exact username match"
        );
        return Ok(None);
    };
    let item = response.directory_items.swap_remove(idx);

    debug!(
        username = username,
        external_user_id = item.user.id,
        "fetched directory item by username"
    );

    Ok(Some(item))
}

/// Check if user exists in DB, if not fetch from individual user API and store
#[instrument(skip(db, client, known_users_cache), fields(username = username, user_id = user_id))]
pub async fn fetch_user_by_username<F: JsonFetcher>(
    db: &DatabaseConnection,
    client: &F,
    instance: &str,
    username: &str,
    user_id: i32,
    known_users_cache: &KnownUsersCache,
) -> Result<()> {
    use entities::{discourse::discourse_users, prelude::*};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

    if !known_users_cache.try_mark_seen(username).await {
        debug!("User already seen in current sync run");
        return Ok(());
    }

    let result: Result<()> = async {
        // First check if user already exists in database
        let existing_user = DiscourseUsers::find()
            .filter(discourse_users::Column::Instance.eq(instance))
            .filter(discourse_users::Column::Username.eq(username))
            .one(db)
            .await?;

        if existing_user.is_some() {
            debug!("User already exists in database");
            return Ok(());
        }

        debug!("User not found in database, fetching from individual user API");

        // Fetch user profile from individual user API
        let path = format!("/u/{}.json", username);
        let user_profile_result = client.get_json::<UserProfileResponse>(&path).await;

        match user_profile_result {
            Ok(response) => {
                let user_profile = response.user;
                debug!(
                    username = username,
                    user_id = user_profile.id,
                    "fetched user profile from API"
                );

                // Create DirectoryItem with only profile data (statistical fields as None)
                let user = User {
                    id: user_profile.id,
                    username: user_profile.username,
                    name: user_profile.name,
                    avatar_template: user_profile.avatar_template,
                    title: user_profile.title,
                    trust_level: user_profile.trust_level,
                    likes_received: None,
                    likes_given: None,
                    topics_entered: None,
                    topic_count: None,
                    post_count: None,
                    posts_read: None,
                    days_visited: None,
                };

                let directory_item = DirectoryItem {
                    id: user_profile.id as u64,
                    likes_received: None,
                    likes_given: None,
                    topics_entered: None,
                    topic_count: None,
                    post_count: None,
                    posts_read: None,
                    days_visited: None,
                    user,
                };

                super::repo::upsert_user(db, instance, &directory_item).await?;
                debug!("Successfully stored user from individual API");
                Ok(())
            }
            Err(e) => {
                warn!("Failed to fetch user from API: {}", e);

                // Create a placeholder user for deleted/missing users to maintain referential integrity
                // Use the actual user_id passed in to match foreign key references
                let placeholder_user = User {
                    id: user_id,
                    username: username.to_string(),
                    name: Some(format!("[Deleted User: {}]", username)),
                    avatar_template: String::new(),
                    title: Some("[Deleted]".to_string()),
                    trust_level: 0,
                    likes_received: None,
                    likes_given: None,
                    topics_entered: None,
                    topic_count: None,
                    post_count: None,
                    posts_read: None,
                    days_visited: None,
                };

                let directory_item = DirectoryItem {
                    id: user_id as u64,
                    likes_received: None,
                    likes_given: None,
                    topics_entered: None,
                    topic_count: None,
                    post_count: None,
                    posts_read: None,
                    days_visited: None,
                    user: placeholder_user,
                };

                super::repo::upsert_user(db, instance, &directory_item).await?;
                debug!(
                    "Created placeholder user for deleted/missing username: {} with ID: {}",
                    username, user_id
                );
                Ok(())
            }
        }
    }
    .await;

    if result.is_err() {
        known_users_cache.unmark(username).await;
    }

    result
}
