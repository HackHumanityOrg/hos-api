use crate::discourse_api::JsonFetcher;
use crate::domain::topics::model::TopicResponse;
use anyhow::Result;
use tracing::{debug, instrument};

#[instrument(skip(client), fields(page = page))]
pub async fn fetch_topics_recent_page<F: JsonFetcher>(
    client: &F,
    page: u32,
) -> Result<TopicResponse> {
    let path = format!("/latest.json?page={}", page);
    let response: TopicResponse = client.get_json(&path).await?;
    debug!(
        page,
        count = response.topic_list.topics.len(),
        "fetched recent topics page"
    );
    Ok(response)
}

#[instrument(skip(client), fields(page = page))]
pub async fn fetch_topics_created_page<F: JsonFetcher>(
    client: &F,
    page: u32,
) -> Result<TopicResponse> {
    let path = format!("/latest.json?order=created&ascending=true&page={}", page);
    let response: TopicResponse = client.get_json(&path).await?;
    debug!(
        page,
        count = response.topic_list.topics.len(),
        "fetched created topics page (sorted by creation date)"
    );
    Ok(response)
}
