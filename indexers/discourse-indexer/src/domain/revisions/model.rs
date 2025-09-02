use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct BodyChanges {
    pub inline: String,
    pub side_by_side: String,
    pub side_by_side_markdown: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TitleChanges {
    pub inline: String,
    pub side_by_side: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Revision {
    // Fields from actual API response
    pub post_id: i32,
    pub created_at: DateTime<Utc>,
    pub username: String,
    pub acting_user_name: Option<String>,
    pub display_username: String,
    pub avatar_template: String,
    pub edit_reason: Option<String>,
    pub body_changes: Option<BodyChanges>,
    pub title_changes: Option<TitleChanges>,
    // Normalized category changes
    pub previous_category_id: Option<i32>,
    pub current_category_id: Option<i32>,
    // Normalized user changes
    pub previous_user_id: Option<i32>,
    pub current_user_id: Option<i32>,
    pub previous_hidden: bool,
    pub current_hidden: bool,
    pub first_revision: i32,
    pub previous_revision: Option<i32>,
    pub current_revision: i32,
    pub next_revision: Option<i32>,
    pub last_revision: i32,
    pub current_version: i32,
    pub version_count: i32,
    pub can_edit: bool,
}
