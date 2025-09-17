use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PostActionUser {
    pub id: i32,
    pub username: String,
    pub name: Option<String>,
    #[serde(default)]
    pub avatar_template: String,
    pub post_url: Option<String>,
    #[serde(default)]
    pub username_lower: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct PostLikeResponse {
    pub post_action_users: Vec<PostActionUser>,
}
