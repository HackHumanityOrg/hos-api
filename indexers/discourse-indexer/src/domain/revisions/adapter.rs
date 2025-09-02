use crate::domain::http::JsonFetcher;
use crate::domain::revisions::model::Revision;
use anyhow::Result;
use tracing::{debug, instrument};

#[instrument(skip(client), fields(post_id = post_id, version = version))]
pub async fn fetch_post_revision<F: JsonFetcher>(
    client: &F,
    post_id: i32,
    version: i32,
) -> Result<Revision> {
    let path = format!("/posts/{}/revisions/{}.json", post_id, version);
    let resp: Revision = client.get_json(&path).await?;
    debug!("fetched post revision");
    Ok(resp)
}
