use crate::domain::http::JsonFetcher;
use crate::domain::posts::model::PostResponse;
use anyhow::Result;
use tracing::{debug, instrument};

#[instrument(skip(client), fields(topic_id = topic_id, page = page))]
pub async fn fetch_posts_page<F: JsonFetcher>(
    client: &F,
    topic_id: i32,
    page: u32,
) -> Result<PostResponse> {
    let path = format!("/t/{}.json?include_raw=true&page={}", topic_id, page + 1);
    let resp: PostResponse = client.get_json(&path).await?;
    let count = resp.post_stream.posts.len();
    debug!(posts = count, posts_count = ?resp.posts_count, "fetched posts page");
    Ok(resp)
}
