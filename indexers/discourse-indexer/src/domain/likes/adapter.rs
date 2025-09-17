use crate::discourse_api::JsonFetcher;
use crate::domain::likes::model::{PostActionUser, PostLikeResponse};
use anyhow::Result;
use tracing::{debug, instrument};

#[instrument(skip(client), fields(post_id = post_id))]
pub async fn fetch_post_likes<F: JsonFetcher>(
    client: &F,
    post_id: i32,
) -> Result<Vec<PostActionUser>> {
    let path = format!(
        "/post_action_users.json?id={}&post_action_type_id=2",
        post_id
    );
    let response: PostLikeResponse = client.get_json(&path).await?;
    debug!(
        post_id,
        count = response.post_action_users.len(),
        "fetched post likes"
    );
    Ok(response.post_action_users)
}
