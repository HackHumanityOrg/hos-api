use crate::discourse_api::JsonFetcher;
use crate::domain::posts::model::PostResponse;
use anyhow::Result;
use tracing::{debug, instrument};

#[instrument(skip(client), fields(topic_id = topic_id, page = page))]
pub async fn fetch_posts_page<F: JsonFetcher>(
    client: &F,
    topic_id: i32,
    page: u32,
) -> Result<PostResponse> {
    // Prefer no explicit page parameter on first fetch. Some Discourse instances
    // reject page=1 while still returning the first chunk without a page value.
    let (path, api_page) = if page == 0 {
        (format!("/t/{}.json?include_raw=true", topic_id), 1)
    } else {
        // Discourse topic pagination is 1-based.
        let next_page = page.saturating_add(1);
        (
            format!("/t/{}.json?page={}&include_raw=true", topic_id, next_page),
            next_page,
        )
    };
    let response: PostResponse = client.get_json(&path).await?;
    debug!(
        topic_id,
        page,
        api_page,
        count = response.post_stream.posts.len(),
        "fetched posts page"
    );
    Ok(response)
}
