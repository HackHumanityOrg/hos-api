use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UserResponse {
    pub directory_items: Vec<DirectoryItem>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DirectoryItem {
    // Required fields from OpenAPI spec for /directory_items.json
    pub id: i32,
    pub likes_received: i32,
    pub likes_given: i32,
    pub topics_entered: i32,
    pub topic_count: i32,
    pub post_count: i32,
    pub posts_read: i32,
    pub days_visited: i32,
    pub user: DirectoryUser,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DirectoryUser {
    pub id: i32,
    pub username: String,
    pub name: Option<String>,
    pub avatar_template: String,
    pub title: Option<String>,
}
