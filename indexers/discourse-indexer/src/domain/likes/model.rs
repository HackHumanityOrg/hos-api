use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PostLikeResponse {
    pub post_action_users: Vec<LikeUser>,
    pub total_rows_post_action_users: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct LikeUser {
    // Fields from actual API response /post_action_users.json
    pub id: i32,
    pub username: String,
}
