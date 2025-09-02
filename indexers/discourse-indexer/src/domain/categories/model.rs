use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CategoryResponse {
    pub category_list: CategoryList,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CategoryList {
    pub categories: Vec<Category>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Category {
    // Required fields from OpenAPI
    pub id: i32,
    pub name: String,
    pub color: String,
    pub text_color: String,
    pub slug: String,
    pub topic_count: i32,
    pub post_count: i32,
    pub position: i32,
    pub description: Option<String>,
    pub description_text: Option<String>,
    pub description_excerpt: Option<String>,
    pub topic_url: Option<String>,
    pub read_restricted: bool,
    pub permission: Option<i32>,
    pub notification_level: Option<i32>,
    #[serde(default)]
    pub can_edit: Option<bool>,
    pub topic_template: Option<String>,
    pub has_children: bool,
    pub subcategory_count: Option<i32>,
    pub sort_order: Option<String>,
    #[serde(deserialize_with = "string_or_bool")]
    pub sort_ascending: Option<String>,
    pub show_subcategory_list: bool,
    pub num_featured_topics: Option<i32>,
    pub default_view: Option<String>,
    pub subcategory_list_style: String,
    pub default_top_period: String,
    pub default_list_filter: String,
    pub minimum_required_tags: Option<i32>,
    pub navigate_to_first_post_after_read: bool,
    pub topics_day: i32,
    pub topics_week: i32,
    pub topics_month: i32,
    pub topics_year: i32,
    pub topics_all_time: i32,
    #[serde(default, deserialize_with = "opt_string_from_any")]
    pub uploaded_logo: Option<String>,
    #[serde(default, deserialize_with = "opt_string_from_any")]
    pub uploaded_logo_dark: Option<String>,
    #[serde(default, deserialize_with = "opt_string_from_any")]
    pub uploaded_background: Option<String>,
    #[serde(default, deserialize_with = "opt_string_from_any")]
    pub uploaded_background_dark: Option<String>,

    // Optional field that may contain subcategories
    #[serde(default)]
    pub subcategory_list: Option<Vec<Category>>,

    // Additional optional field
    pub is_uncategorized: Option<bool>,
}

fn string_or_bool<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    use serde_json::Value;

    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(s) => Ok(Some(s)),
        Value::Bool(b) => Ok(Some(b.to_string())),
        Value::Null => Ok(None),
        _ => Ok(Some(value.to_string())),
    }
}

fn opt_string_from_any<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;
    use serde_json::Value;

    let value = Value::deserialize(deserializer)?;
    match value {
        // Already a string
        Value::String(s) => Ok(Some(s)),
        Value::Null => Ok(None),
        // If the API returns an object with a `url` field, extract that string
        Value::Object(map) => match map.get("url") {
            Some(Value::String(u)) => Ok(Some(u.clone())),
            _ => Ok(None),
        },
        // Numbers, arrays, bools: ignore
        _ => Ok(None),
    }
}
