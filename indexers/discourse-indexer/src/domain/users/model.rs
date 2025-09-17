use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct User {
    pub id: i32,
    pub username: String,
    pub name: Option<String>,
    #[serde(default)]
    pub avatar_template: String,
    pub title: Option<String>,
    #[serde(default)]
    pub trust_level: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub likes_received: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub likes_given: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics_entered: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_count: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub posts_read: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days_visited: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DirectoryItem {
    pub id: u64,
    pub likes_received: Option<u64>,
    pub likes_given: Option<u64>,
    pub topics_entered: Option<u64>,
    pub topic_count: Option<u64>,
    pub post_count: Option<u64>,
    pub posts_read: Option<u64>,
    pub days_visited: Option<u64>,
    pub user: User,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Meta {
    pub last_updated_at: String,
    pub total_rows_directory_items: u64,
    pub load_more_directory_items: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UserResponse {
    pub directory_items: Vec<DirectoryItem>,
    pub meta: Meta,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UserProfile {
    pub id: i32,
    pub username: String,
    pub name: Option<String>,
    #[serde(default)]
    pub avatar_template: String,
    pub title: Option<String>,
    #[serde(default)]
    pub trust_level: i32,
    pub bio_raw: Option<String>,
    pub bio_cooked: Option<String>,
    pub bio_excerpt: Option<String>,
    #[serde(default)]
    pub profile_view_count: i32,
    pub location: Option<String>,
    pub website: Option<String>,
    pub website_name: Option<String>,
    #[serde(default)]
    pub can_edit: bool,
    #[serde(default)]
    pub can_edit_username: bool,
    #[serde(default)]
    pub can_edit_email: bool,
    #[serde(default)]
    pub can_edit_name: bool,
    pub flair_name: Option<String>,
    pub flair_url: Option<String>,
    pub flair_bg_color: Option<String>,
    pub flair_color: Option<String>,
    pub primary_group_name: Option<String>,
    pub primary_group_id: Option<i32>,
    #[serde(default)]
    pub moderator: bool,
    #[serde(default)]
    pub admin: bool,
    pub last_posted_at: Option<String>,
    pub last_seen_at: Option<String>,
    pub created_at: Option<String>,
    pub time_read: Option<i64>,
    #[serde(default)]
    pub badge_count: i32,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct UserProfileResponse {
    pub user: UserProfile,
}
