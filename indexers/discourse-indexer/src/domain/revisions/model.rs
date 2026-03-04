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
    #[serde(default)]
    pub body_changes: Option<BodyChanges>,
    pub title_changes: Option<TitleChanges>,
}

impl Revision {
    pub fn get_raw_markdown_before(&self) -> String {
        self.body_changes
            .as_ref()
            .and_then(|changes| extract_raw_markdown_before(&changes.side_by_side_markdown))
            .unwrap_or_default()
    }

    pub fn get_raw_markdown_after(&self) -> String {
        self.body_changes
            .as_ref()
            .and_then(|changes| extract_raw_markdown_after(&changes.side_by_side_markdown))
            .unwrap_or_default()
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
    if let Some((before, _)) = extract_markdown_columns(content) {
        return Some(before.join("\n\n"));
    }

    Some(clean_body_markdown(content))
}

fn extract_raw_markdown_after(content: &str) -> Option<String> {
    if let Some((_, after)) = extract_markdown_columns(content) {
        return Some(after.join("\n\n"));
    }

    Some(clean_body_markdown(content))
}

fn clean_body_markdown(markdown: &str) -> String {
    let mut cleaned = markdown.replace("\r\n", "\n");
    for (target, replacement) in [
        ("<del>", ""),
        ("</del>", ""),
        ("<ins>", ""),
        ("</ins>", ""),
        (r#" class=\"diff-ins\""#, ""),
        (r#" class=\"diff-del\""#, ""),
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
        let Some(before_cell) = cells.next() else {
            continue;
        };
        let Some(after_cell) = cells.next() else {
            continue;
        };
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_markdown_columns_and_cleans_diff_markup() {
        let html = r#"
            <table>
              <tr>
                <td><del>before</del> &amp; text</td>
                <td><ins>after</ins> &amp; text</td>
              </tr>
            </table>
        "#;

        let (before, after) = extract_markdown_columns(html).expect("columns should parse");
        assert_eq!(before, vec!["before & text".to_string()]);
        assert_eq!(after, vec!["after & text".to_string()]);
    }

    #[test]
    fn falls_back_to_cleaned_body_when_no_diff_table_exists() {
        let inline = "<del>old</del> and <ins>new</ins> &amp; more";

        let before = extract_raw_markdown_before(inline).expect("before fallback");
        let after = extract_raw_markdown_after(inline).expect("after fallback");

        assert_eq!(before, "old and new & more");
        assert_eq!(after, "old and new & more");
    }

    #[test]
    fn extracts_title_columns_from_revision_html() {
        let html = r#"
            <div class="revision-content">
                <div><del>Old &amp; Title</del></div>
                <div><ins>New &amp; Title</ins></div>
            </div>
        "#;

        let (before, after) = extract_title_columns(html).expect("title columns should parse");
        assert_eq!(before, "Old & Title");
        assert_eq!(after, "New & Title");
    }

    #[test]
    fn parses_real_revision_fixture_and_exposes_raw_helpers() {
        let raw = include_str!("../../../tests/fixtures/meta/post_326680_revision_2.json");
        let revision: Revision = serde_json::from_str(raw).expect("fixture should parse");

        let before = revision.get_raw_markdown_before();
        let after = revision.get_raw_markdown_after();

        assert!(before.contains("Repo:"));
        assert!(after.contains("More notes:"));
        assert!(!before.contains("<del>"));
        assert!(!after.contains("<ins>"));
    }

    #[test]
    fn parses_revision_with_null_body_changes() {
        let raw = r#"
        {
          "created_at": "2021-01-08T16:42:31.934Z",
          "post_id": 676,
          "previous_revision": 3,
          "current_revision": 4,
          "next_revision": 5,
          "current_version": 4,
          "username": "illia",
          "edit_reason": null,
          "body_changes": null,
          "title_changes": {
            "inline": "<div class=\"inline-diff\"><div>EVM Runtime native token</div></div>",
            "side_by_side": "<div class=\"revision-content --previous\"><div>EVM Runtime native token</div></div><div class=\"revision-content --current\"><div>EVM Runtime native token</div></div>"
          }
        }
        "#;

        let revision: Revision = serde_json::from_str(raw).expect("null body_changes should parse");

        assert!(revision.body_changes.is_none());
        assert_eq!(revision.get_raw_markdown_before(), "");
        assert_eq!(revision.get_raw_markdown_after(), "");
    }
}
