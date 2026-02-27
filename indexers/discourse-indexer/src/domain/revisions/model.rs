use chrono::{DateTime, Utc};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct BodyChanges {
    pub inline: String,
    pub side_by_side: String,
    pub side_by_side_markdown: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TitleChanges {
    pub inline: String,
    pub side_by_side: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Revision {
    pub created_at: DateTime<Utc>,
    pub post_id: i32,
    #[serde(default)]
    pub previous_hidden: bool,
    #[serde(default)]
    pub current_hidden: bool,
    #[serde(default)]
    pub first_revision: i32,
    pub previous_revision: Option<i32>,
    pub current_revision: i32,
    pub next_revision: Option<i32>,
    #[serde(default)]
    pub last_revision: i32,
    pub current_version: i32,
    #[serde(default)]
    pub version_count: i32,
    pub username: String,
    #[serde(default)]
    pub display_username: String,
    #[serde(default)]
    pub avatar_template: String,
    pub edit_reason: Option<String>,
    pub body_changes: BodyChanges,
    pub title_changes: Option<TitleChanges>,
}

impl Revision {
    pub fn get_raw_markdown_before(&self) -> String {
        extract_raw_markdown_before(&self.body_changes.side_by_side_markdown).unwrap_or_default()
    }

    pub fn get_raw_markdown_after(&self) -> String {
        extract_raw_markdown_after(&self.body_changes.side_by_side_markdown).unwrap_or_default()
    }
}

impl TitleChanges {
    pub fn get_raw_title_before(&self) -> String {
        extract_title_before_side_by_side(&self.side_by_side).unwrap_or_default()
    }

    pub fn get_raw_title_after(&self) -> String {
        extract_title_after_side_by_side(&self.side_by_side).unwrap_or_default()
    }
}

fn extract_raw_markdown_before(content: &str) -> Option<String> {
    let (before, _) = extract_markdown_columns(content)?;
    Some(before.join("\n\n"))
}

fn extract_raw_markdown_after(content: &str) -> Option<String> {
    let (_, after) = extract_markdown_columns(content)?;
    Some(after.join("\n\n"))
}

fn clean_body_markdown(markdown: &str) -> String {
    let mut cleaned = markdown.replace("\r\n", "\n");
    for (target, replacement) in [
        ("<del>", ""),
        ("</del>", ""),
        ("<ins>", ""),
        ("</ins>", ""),
        ("&amp;", "&"),
        ("&lt;", "<"),
        ("&gt;", ">"),
        ("&quot;", "\""),
        ("&#x27;", "'"),
        ("&#39;", "'"),
        ("&nbsp;", " "),
    ] {
        cleaned = cleaned.replace(target, replacement);
    }
    cleaned.trim().to_string()
}

fn extract_title_before_side_by_side(content: &str) -> Option<String> {
    let (before, _) = extract_title_columns(content)?;
    Some(before)
}

fn extract_title_after_side_by_side(content: &str) -> Option<String> {
    let (_, after) = extract_title_columns(content)?;
    Some(after)
}

fn clean_title_markdown(title: &str) -> String {
    title
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("<del>", "")
        .replace("</del>", "")
        .replace("<ins>", "")
        .replace("</ins>", "")
        .replace(r#" class=\"diff-ins\""#, "")
        .replace(r#" class=\"diff-del\""#, "")
        .trim()
        .to_string()
}

fn extract_markdown_columns(content: &str) -> Option<(Vec<String>, Vec<String>)> {
    let document = Html::parse_fragment(content);
    let row_selector = Selector::parse("tr").ok()?;
    let cell_selector = Selector::parse("td").ok()?;

    let mut before = Vec::new();
    let mut after = Vec::new();

    for row in document.select(&row_selector) {
        let mut cells = row.select(&cell_selector);
        let before_cell = cells.next()?;
        let after_cell = cells.next()?;
        let before_html = before_cell.inner_html();
        let after_html = after_cell.inner_html();
        before.push(clean_body_markdown(&before_html));
        after.push(clean_body_markdown(&after_html));
    }

    if before.is_empty() {
        None
    } else {
        Some((before, after))
    }
}

fn extract_title_columns(content: &str) -> Option<(String, String)> {
    let document = Html::parse_fragment(content);
    let section_selector = Selector::parse("div.revision-content > div").ok()?;
    let mut sections = document.select(&section_selector);
    let before = clean_title_markdown(sections.next()?.inner_html().trim());
    let after = clean_title_markdown(sections.next()?.inner_html().trim());
    Some((before, after))
}
