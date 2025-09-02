use crate::domain::categories::model::{Category, CategoryResponse};
use crate::domain::http::JsonFetcher;
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
        flatten_categories(&r.category_list.categories, &mut all);
        page += 1;
    }
    debug!(total = all.len(), "flattened categories total");
    Ok(all)
}

fn flatten_categories(src: &[Category], out: &mut Vec<Category>) {
    for c in src.iter().cloned() {
        if let Some(children) = c.subcategory_list.clone() {
            let c_no_children = c.clone();
            out.push(c_no_children);
            flatten_categories(&children, out);
        } else {
            out.push(c);
        }
    }
}
