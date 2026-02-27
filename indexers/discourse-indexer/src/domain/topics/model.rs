use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TopicResponse {
    pub topic_list: TopicList,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TopicList {
    pub topics: Vec<Topic>,
    pub per_page: Option<i32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Topic {
    pub id: i32,
    pub title: String,
    pub fancy_title: Option<String>,
    pub slug: String,
    pub posts_count: i32,
    pub reply_count: i32,
    pub highest_post_number: i32,
    pub image_url: Option<String>,
    #[serde(with = "date_format")]
    pub created_at: DateTime<Utc>,
    #[serde(with = "date_format")]
    pub last_posted_at: DateTime<Utc>,
    pub bumped: Option<bool>,
    #[serde(with = "date_format")]
    pub bumped_at: DateTime<Utc>,
    pub archetype: String,
    pub pinned: Option<bool>,
    pub visible: Option<bool>,
    pub closed: Option<bool>,
    pub archived: Option<bool>,
    pub views: i32,
    pub like_count: i32,
    pub has_summary: Option<bool>,
    pub last_poster_username: Option<String>,
    pub category_id: i32,
    pub pinned_globally: Option<bool>,
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
    fn topic_model_serializes_and_deserializes_rfc3339_dates() {
        let payload = json!({
            "id": 7,
            "title": "Example topic",
            "fancy_title": "Example topic",
            "slug": "example-topic",
            "posts_count": 2,
            "reply_count": 1,
            "highest_post_number": 2,
            "image_url": null,
            "created_at": "2026-01-01T00:00:00Z",
            "last_posted_at": "2026-01-01T01:00:00Z",
            "bumped": true,
            "bumped_at": "2026-01-01T01:00:00Z",
            "archetype": "regular",
            "pinned": false,
            "visible": true,
            "closed": false,
            "archived": false,
            "views": 100,
            "like_count": 3,
            "has_summary": false,
            "last_poster_username": "alice",
            "category_id": 10,
            "pinned_globally": false
        });

        let topic: Topic = serde_json::from_value(payload).expect("topic should deserialize");
        assert_eq!(topic.id, 7);
        assert_eq!(topic.category_id, 10);

        let serialized = serde_json::to_value(&topic).expect("topic should serialize");
        assert_eq!(serialized["created_at"], "2026-01-01T00:00:00+00:00");
        assert_eq!(serialized["bumped_at"], "2026-01-01T01:00:00+00:00");
    }

    #[test]
    fn topic_date_format_module_roundtrips() {
        let dt = Utc.with_ymd_and_hms(2026, 3, 4, 5, 6, 7).unwrap();
        let encoded = serde_json::to_string(&dt.to_rfc3339()).expect("encode date string");

        let mut de = serde_json::Deserializer::from_str(&encoded);
        let parsed = date_format::deserialize(&mut de).expect("decode rfc3339");
        assert_eq!(parsed, dt);
    }
}
