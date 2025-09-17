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
    let path = format!("/t/{}.json?page={}&include_raw=true", topic_id, page);
    let response: PostResponse = client.get_json(&path).await?;
    debug!(
        topic_id,
        page,
        count = response.post_stream.posts.len(),
        "fetched posts page"
    );
    Ok(response)
}
