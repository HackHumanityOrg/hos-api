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
    // Required fields from API
    pub id: i32,
    pub name: String,
    pub color: String,
    pub text_color: String,
    pub slug: String,
    pub topic_count: i32,
    pub post_count: i32,
    #[serde(default)]
    pub position: i32,
    pub description: Option<String>,
    pub description_text: Option<String>,
    pub description_excerpt: Option<String>,
    pub topic_url: Option<String>,
    pub read_restricted: Option<bool>,
    pub notification_level: Option<i32>,
    pub topic_template: Option<String>,
    #[serde(default)]
    pub has_children: bool,
    pub subcategory_count: Option<i32>,
    pub sort_order: Option<String>,
    #[serde(default, deserialize_with = "string_or_bool")]
    pub sort_ascending: Option<String>,
    #[serde(default)]
    pub show_subcategory_list: bool,
    pub num_featured_topics: Option<i32>,
    pub default_view: Option<String>,
    #[serde(default)]
    pub subcategory_list_style: String,
    #[serde(default)]
    pub default_top_period: String,
    #[serde(default)]
    pub default_list_filter: String,
    pub minimum_required_tags: Option<i32>,
    #[serde(default)]
    pub navigate_to_first_post_after_read: bool,
    pub topics_day: i32,
    pub topics_week: i32,
    pub topics_month: i32,
    pub topics_year: i32,
    pub topics_all_time: i32,

    // Optional field that may contain subcategories
    #[serde(default)]
    pub subcategory_list: Option<Vec<Category>>,

    // Additional optional field
    pub is_uncategorized: Option<bool>,

    #[serde(skip)]
    pub parent_id: Option<i32>,
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
