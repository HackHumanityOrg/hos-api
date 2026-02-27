use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Post {
    pub id: i32,
    pub name: Option<String>,
    pub username: String,
    #[serde(default)]
    pub avatar_template: String,
    #[serde(with = "date_format")]
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub cooked: Option<String>,
    #[serde(default)]
    pub raw: Option<String>,
    pub post_number: i32,
    #[serde(default)]
    pub post_type: i32,
    #[serde(with = "date_format")]
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub reply_count: i32,
    pub reply_to_post_number: Option<i32>,
    #[serde(default)]
    pub quote_count: i32,
    #[serde(default)]
    pub incoming_link_count: i32,
    #[serde(default)]
    pub reads: i32,
    #[serde(default)]
    pub readers_count: i32,
    #[serde(default)]
    pub score: f64,
    pub topic_id: i32,
    pub topic_slug: String,
    pub display_username: Option<String>,
    pub primary_group_name: Option<String>,
    pub flair_name: Option<String>,
    pub flair_url: Option<String>,
    pub flair_bg_color: Option<String>,
    pub flair_color: Option<String>,
    #[serde(default = "default_version")]
    pub version: i32,
    #[serde(default)]
    pub can_view_edit_history: bool,
    pub user_id: i32,
    #[serde(default)]
    pub actions_summary: Vec<ActionSummary>,
    #[serde(default)]
    pub deleted: bool,
}

fn default_version() -> i32 {
    1
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ActionSummary {
    pub id: u64,
    pub count: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PostResponse {
    pub post_stream: PostStream,
    pub posts_count: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PostStream {
    pub posts: Vec<Post>,
}

mod date_format {
    use chrono::{DateTime, Utc};
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(date: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&date.to_rfc3339())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<DateTime<Utc>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&s)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use serde_json::json;

    #[test]
    fn post_model_deserializes_defaults_and_serializes_dates() {
        let payload = json!({
            "id": 42,
            "name": "Alice",
            "username": "alice",
            "avatar_template": "/user_avatar/example/{size}/1_2.png",
            "created_at": "2026-01-01T12:00:00Z",
            "updated_at": "2026-01-01T12:01:00Z",
            "post_number": 1,
            "reply_to_post_number": null,
            "topic_id": 99,
            "topic_slug": "example-topic",
            "display_username": "Alice",
            "primary_group_name": null,
            "flair_name": null,
            "flair_url": null,
            "flair_bg_color": null,
            "flair_color": null,
            "user_id": 7
        });

        let post: Post = serde_json::from_value(payload).expect("post should deserialize");
        assert_eq!(post.id, 42);
        assert_eq!(post.post_type, 0);
        assert_eq!(post.version, 1);
        assert!(post.actions_summary.is_empty());
        assert!(!post.deleted);

        let serialized = serde_json::to_value(&post).expect("post should serialize");
        assert_eq!(serialized["created_at"], "2026-01-01T12:00:00+00:00");
        assert_eq!(serialized["updated_at"], "2026-01-01T12:01:00+00:00");
    }

    #[test]
    fn date_format_module_handles_rfc3339_roundtrip() {
        let dt = Utc.with_ymd_and_hms(2026, 2, 3, 4, 5, 6).unwrap();
        let encoded = serde_json::to_string(&dt.with_timezone(&Utc).to_rfc3339())
            .expect("encode date string");

        let mut de = serde_json::Deserializer::from_str(&encoded);
        let parsed = date_format::deserialize(&mut de).expect("decode rfc3339");
        assert_eq!(parsed, dt);
    }
}
