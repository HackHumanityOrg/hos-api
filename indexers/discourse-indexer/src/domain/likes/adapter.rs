use crate::discourse_api::JsonFetcher;
use crate::domain::likes::model::{PostActionUser, PostLikeResponse};
use anyhow::Result;
use std::collections::HashSet;
use tracing::{debug, instrument};

const LIKE_PAGE_SIZE: u32 = 50;
const MAX_LIKES_PAGES: u32 = 1_000;

#[instrument(skip(client), fields(post_id = post_id))]
pub async fn fetch_post_likes<F: JsonFetcher>(
    client: &F,
    post_id: i32,
) -> Result<Vec<PostActionUser>> {
    let mut all_users = Vec::new();
    let mut seen_user_ids = HashSet::new();
    let mut prev_page_ids: Option<Vec<i32>> = None;
    let mut page = 0;

    loop {
        if page >= MAX_LIKES_PAGES {
            break;
        }

        let path = format!(
            "/post_action_users.json?id={}&post_action_type_id=2&page={}&limit={}",
            post_id, page, LIKE_PAGE_SIZE
        );
        let response: PostLikeResponse = client.get_json(&path).await?;

        let page_users = response.post_action_users;
        let page_count = page_users.len();
        if page_users.is_empty() {
            break;
        }

        let page_ids: Vec<i32> = page_users.iter().map(|u| u.id).collect();
        if let Some(prev) = &prev_page_ids {
            if prev == &page_ids {
                debug!(post_id, page, "detected repeated likes page, stopping");
                break;
            }
        }
        prev_page_ids = Some(page_ids);

        for user in page_users {
            if seen_user_ids.insert(user.id) {
                all_users.push(user);
            }
        }

        debug!(
            post_id,
            page,
            fetched = all_users.len(),
            total_rows = response.total_rows_post_action_users,
            "fetched paged post likes"
        );

        if let Some(total_rows) = response.total_rows_post_action_users {
            if total_rows >= 0 && all_users.len() >= total_rows as usize {
                break;
            }
        }

        if response.total_rows_post_action_users.is_none() && page_count < LIKE_PAGE_SIZE as usize {
            break;
        }

        page += 1;
    }

    debug!(post_id, count = all_users.len(), "fetched post likes");
    Ok(all_users)
}
