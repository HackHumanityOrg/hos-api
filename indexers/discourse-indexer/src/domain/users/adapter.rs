use crate::domain::http::JsonFetcher;
use crate::domain::users::model::{DirectoryItem, UserProfileResponse, UserResponse};
use anyhow::Result;
use tracing::{debug, instrument};

#[instrument(skip(client), fields(recent = recent, page_limit = page_limit, max_pages = max_pages))]
pub async fn fetch_users_directory<F: JsonFetcher>(
    client: &F,
    recent: bool,
    page_limit: u32,
    max_pages: u32,
) -> Result<Vec<DirectoryItem>> {
    let order = if recent { "desc" } else { "asc" };
    let mut page: u32 = 0;
    let mut all: Vec<DirectoryItem> = Vec::new();
    loop {
        if page > max_pages || page > page_limit {
            break;
        }
        let path = format!(
            "/directory_items.json?page={}&order={}&period=all",
            page, order
        );
        let r: UserResponse = client.get_json(&path).await?;
        debug!(
            page,
            count = r.directory_items.len(),
            "fetched directory users"
        );
        if r.directory_items.is_empty() {
            break;
        }
        all.extend(r.directory_items);
        page += 1;
    }
    debug!(total = all.len(), "fetched total directory users");
    Ok(all)
}

/// Fetch a single user profile by username using `/u/{username}.json`.
/// This endpoint is useful to ensure a user exists and retrieve identity fields.
#[instrument(skip(client), fields(username = username))]
pub async fn fetch_user_profile<F: JsonFetcher>(
    client: &F,
    username: &str,
) -> Result<UserProfileResponse> {
    let path = format!("/u/{}.json", username);
    let resp: UserProfileResponse = client.get_json(&path).await?;
    debug!(id = resp.user.id, username = %resp.user.username, "fetched user profile");
    Ok(resp)
}
