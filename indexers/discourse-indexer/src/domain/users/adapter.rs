use crate::discourse_api::JsonFetcher;
use crate::domain::users::model::{DirectoryItem, User, UserProfileResponse, UserResponse};
use anyhow::Result;
use sea_orm::DatabaseConnection;
use tracing::{debug, instrument, warn};

#[instrument(skip(client), fields(recent = recent, page_limit = page_limit, max_pages = max_pages))]
pub async fn fetch_users_directory<F: JsonFetcher>(
    client: &F,
    recent: bool,
    page_limit: u32,
    max_pages: u32,
) -> Result<Vec<DirectoryItem>> {
    let mut all_users = Vec::new();
    let mut page = 0;

    loop {
        if page >= max_pages {
            break;
        }

        let order = if recent {
            "posts_read"
        } else {
            "likes_received"
        };
        let path = format!(
            "/directory_items.json?period=all&order={}&page={}",
            order, page
        );

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

        all_users.extend(response.directory_items);

        if page >= page_limit {
            break;
        }

        page += 1;
    }

    debug!(total = all_users.len(), "fetched users directory total");
    Ok(all_users)
}


/// Check if user exists in DB, if not fetch from individual user API and store
#[instrument(skip(db, client), fields(username = username, user_id = user_id))]
pub async fn fetch_user_by_username<F: JsonFetcher>(
    db: &DatabaseConnection,
    client: &F,
    instance: &str,
    username: &str,
    user_id: i32,
) -> Result<()> {
    use entities::{discourse_users, prelude::*};
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

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
    let path = format!("/users/{}.json", username);
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
