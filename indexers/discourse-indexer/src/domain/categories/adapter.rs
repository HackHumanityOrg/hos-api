use crate::discourse_api::JsonFetcher;
use crate::domain::categories::model::{Category, CategoryResponse};
use anyhow::Result;
use tracing::{debug, instrument};

#[instrument(skip(client), fields(max_pages = max_pages))]
pub async fn fetch_categories_all<F: JsonFetcher>(
    client: &F,
    max_pages: u32,
) -> Result<Vec<Category>> {
    let mut all = Vec::new();
    let mut page: u32 = 0;
    loop {
        if page > max_pages {
            break;
        }
        let path = format!("/categories.json?include_subcategories=true&page={}", page);
        let r: CategoryResponse = client.get_json(&path).await?;
        debug!(
            page,
            count = r.category_list.categories.len(),
            "fetched categories page"
        );
        if r.category_list.categories.is_empty() {
            break;
        }
        flatten_categories_with_parent(&r.category_list.categories, &mut all, None);
        page += 1;
    }
    debug!(total = all.len(), "flattened categories total");
    Ok(all)
}

fn flatten_categories_with_parent(
    src: &[Category],
    out: &mut Vec<Category>,
    parent_id: Option<i32>,
) {
    for c in src.iter().cloned() {
        let mut category = c.clone();
        category.parent_id = parent_id;

        if let Some(children) = c.subcategory_list.clone() {
            category.subcategory_list = None;
            out.push(category.clone());
            flatten_categories_with_parent(&children, out, Some(category.id));
        } else {
            out.push(category);
        }
    }
}
