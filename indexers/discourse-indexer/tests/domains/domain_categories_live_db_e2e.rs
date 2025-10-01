use anyhow::Result;
use discourse_indexer::domain::categories::{adapter, repo};
use discourse_indexer::domain::instances;
use entities::{discourse::discourse_categories, prelude::DiscourseCategories};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
use std::collections::HashMap;
use test_support::{init_test_tracing, TestDatabase};

#[path = "../support/mod.rs"]
mod support;
use support::FixtureFetcher;

const INSTANCE_KEY: &str = "meta";
const INSTANCE_URL: &str = "https://meta.discourse.org";

#[tokio::test]
async fn fetch_categories_from_fixture_flattens_hierarchy() -> Result<()> {
    let fetcher = FixtureFetcher::new(&[
        (
            "/categories.json?include_subcategories=true&page=0",
            "meta/categories_page_0.json",
        ),
        (
            "/categories.json?include_subcategories=true&page=1",
            "meta/categories_page_1.json",
        ),
        (
            "/categories.json?include_subcategories=true&page=2",
            "meta/categories_page_2.json",
        ),
    ])?;

    let categories = adapter::fetch_categories_all(&fetcher, 5).await?;

    assert!(categories.len() > 40, "expected flattened category list");

    let general = categories
        .iter()
        .find(|c| c.id == 10)
        .expect("General category present");
    assert!(general.parent_id.is_none());
    assert!(general.subcategory_list.is_none());

    let comparison = categories
        .iter()
        .find(|c| c.id == 63)
        .expect("Comparison subcategory present");
    assert_eq!(comparison.parent_id, Some(35));
    assert!(comparison.subcategory_list.is_none());

    Ok(())
}

#[tokio::test]
async fn category_upsert_persists_every_column() -> Result<()> {
    init_test_tracing();
    let db = TestDatabase::new().await;
    let conn = db.db();

    instances::repo::register_instance(conn.as_ref(), INSTANCE_KEY, INSTANCE_URL).await?;

    let fetcher = FixtureFetcher::new(&[(
        "/categories.json?include_subcategories=true&page=0",
        "meta/categories_page_0.json",
    )])?;
    let categories = adapter::fetch_categories_all(&fetcher, 0).await?;

    for category in &categories {
        repo::upsert_category(conn.as_ref(), INSTANCE_KEY, category).await?;
    }

    let stored = DiscourseCategories::find()
        .filter(discourse_categories::Column::Instance.eq(INSTANCE_KEY))
        .order_by_asc(discourse_categories::Column::ExternalId)
        .all(conn.as_ref())
        .await?;

    assert_eq!(stored.len(), categories.len());

    let expected: HashMap<i32, _> = categories.iter().map(|c| (c.id, c)).collect();

    for row in stored {
        let src = expected
            .get(&row.external_id)
            .expect("category fixture present");
        assert_eq!(row.instance, INSTANCE_KEY);
        assert_eq!(row.external_id, src.id);
        assert_eq!(row.name, src.name);
        assert_eq!(row.color, src.color);
        assert_eq!(row.text_color, src.text_color);
        assert_eq!(row.slug, src.slug);
        assert_eq!(row.topic_count, src.topic_count);
        assert_eq!(row.post_count, src.post_count);
        assert_eq!(row.description.as_deref(), src.description.as_deref());
        assert_eq!(
            row.description_text.as_deref(),
            src.description_text.as_deref()
        );
        assert_eq!(row.topics_day, Some(src.topics_day));
        assert_eq!(row.topics_week, Some(src.topics_week));
        assert_eq!(row.topics_month, Some(src.topics_month));
        assert_eq!(row.topics_year, Some(src.topics_year));
        assert_eq!(row.topics_all_time, Some(src.topics_all_time));
        assert_eq!(row.parent_category_id, src.parent_id);
    }

    // Run idempotent upsert and ensure counts remain stable.
    for category in &categories {
        repo::upsert_category(conn.as_ref(), INSTANCE_KEY, category).await?;
    }

    let stored_after_second = DiscourseCategories::find()
        .filter(discourse_categories::Column::Instance.eq(INSTANCE_KEY))
        .order_by_asc(discourse_categories::Column::ExternalId)
        .all(conn.as_ref())
        .await?;
    assert_eq!(stored_after_second.len(), categories.len());

    // Mutate a description to verify conflict updates apply.
    let mut categories_updated = categories.clone();
    if let Some(first) = categories_updated.first_mut() {
        let new_description = first
            .description
            .clone()
            .unwrap_or_else(|| "Updated category description".to_string());
        first.description = Some(format!("{new_description} (updated)"));
        first.description_text = first.description.clone();
    }
    for category in &categories_updated {
        repo::upsert_category(conn.as_ref(), INSTANCE_KEY, category).await?;
    }

    let stored_after_update = DiscourseCategories::find()
        .filter(discourse_categories::Column::Instance.eq(INSTANCE_KEY))
        .order_by_asc(discourse_categories::Column::ExternalId)
        .all(conn.as_ref())
        .await?;
    assert_eq!(stored_after_update.len(), categories.len());

    if let Some(first_updated) = categories_updated.first() {
        let stored_first = stored_after_update
            .iter()
            .find(|row| row.external_id == first_updated.id)
            .expect("updated category stored");
        assert_eq!(
            stored_first.description.as_deref(),
            first_updated.description.as_deref()
        );
        assert_eq!(
            stored_first.description_text.as_deref(),
            first_updated.description_text.as_deref()
        );
    }

    Ok(())
}
