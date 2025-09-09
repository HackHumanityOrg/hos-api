use crate::domain::categories::model::Category;
use crate::domain::util::{bool_i8, opt_bool_to_i8};
use anyhow::Result;
use db_core::with_db_retry;
use entities::discourse_categories;
use sea_orm::sea_query::OnConflict as OrmOnConflict;
use sea_orm::{ActiveValue::Set, EntityTrait};
use tracing::{debug, instrument};

#[instrument(skip(db, c), fields(instance = instance, external_id = c.id))]
pub async fn upsert_category(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    c: &Category,
) -> Result<()> {
    let am = discourse_categories::ActiveModel {
        instance: Set(instance.to_string()),
        external_id: Set(c.id),
        name: Set(c.name.clone()),
        color: Set(c.color.clone()),
        text_color: Set(c.text_color.clone()),
        slug: Set(c.slug.clone()),
        topic_count: Set(c.topic_count),
        post_count: Set(c.post_count),
        position: Set(c.position),
        description: Set(c.description.clone()),
        description_text: Set(c.description_text.clone()),
        description_excerpt: Set(c.description_excerpt.clone()),
        topic_url: Set(c.topic_url.clone()),
        topic_template: Set(c.topic_template.clone()),
        has_children: Set(bool_i8(c.has_children)),
        subcategory_count: Set(c.subcategory_count),
        sort_order: Set(c.sort_order.clone()),
        num_featured_topics: Set(c.num_featured_topics.unwrap_or(3)),
        default_view: Set(c.default_view.clone()),
        subcategory_list_style: Set(c.subcategory_list_style.clone()),
        default_top_period: Set(c.default_top_period.clone()),
        default_list_filter: Set(c.default_list_filter.clone()),
        minimum_required_tags: Set(c.minimum_required_tags.unwrap_or(0)),
        topics_day: Set(c.topics_day),
        topics_week: Set(c.topics_week),
        topics_month: Set(c.topics_month),
        topics_year: Set(c.topics_year),
        topics_all_time: Set(c.topics_all_time),
        uploaded_logo: Set(c.uploaded_logo.clone()),
        uploaded_logo_dark: Set(c.uploaded_logo_dark.clone()),
        uploaded_background: Set(c.uploaded_background.clone()),
        uploaded_background_dark: Set(c.uploaded_background_dark.clone()),
        is_uncategorized: Set(opt_bool_to_i8(c.is_uncategorized)),
        ..Default::default()
    };
    with_db_retry(|| async {
        discourse_categories::Entity::insert(am.clone())
            .on_conflict(
                OrmOnConflict::columns([
                    discourse_categories::Column::Instance,
                    discourse_categories::Column::ExternalId,
                ])
                .update_columns([
                    discourse_categories::Column::Name,
                    discourse_categories::Column::Color,
                    discourse_categories::Column::TextColor,
                    discourse_categories::Column::Slug,
                    discourse_categories::Column::TopicCount,
                    discourse_categories::Column::PostCount,
                    discourse_categories::Column::Position,
                    discourse_categories::Column::Description,
                    discourse_categories::Column::DescriptionText,
                    discourse_categories::Column::DescriptionExcerpt,
                    discourse_categories::Column::TopicUrl,
                    discourse_categories::Column::TopicTemplate,
                    discourse_categories::Column::HasChildren,
                    discourse_categories::Column::SubcategoryCount,
                    discourse_categories::Column::SortOrder,
                    discourse_categories::Column::NumFeaturedTopics,
                    discourse_categories::Column::DefaultView,
                    discourse_categories::Column::SubcategoryListStyle,
                    discourse_categories::Column::DefaultTopPeriod,
                    discourse_categories::Column::DefaultListFilter,
                    discourse_categories::Column::MinimumRequiredTags,
                    discourse_categories::Column::TopicsDay,
                    discourse_categories::Column::TopicsWeek,
                    discourse_categories::Column::TopicsMonth,
                    discourse_categories::Column::TopicsYear,
                    discourse_categories::Column::TopicsAllTime,
                    discourse_categories::Column::UploadedLogo,
                    discourse_categories::Column::UploadedLogoDark,
                    discourse_categories::Column::UploadedBackground,
                    discourse_categories::Column::UploadedBackgroundDark,
                    discourse_categories::Column::IsUncategorized,
                ])
                .to_owned(),
            )
            .exec(db)
            .await
            .or_else(|err| match err {
                sea_orm::DbErr::RecordNotInserted => {
                    Ok(sea_orm::InsertResult { last_insert_id: 0 })
                }
                _ => Err(err),
            })
    })
    .await?;
    debug!("upserted category record");
    Ok(())
}
