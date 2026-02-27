use crate::domain::categories::model::Category;
use anyhow::Result;
use entities::discourse::discourse_categories;
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
        description: Set(c.description.clone()),
        description_text: Set(c.description_text.clone()),
        topics_day: Set(Some(c.topics_day)),
        topics_week: Set(Some(c.topics_week)),
        topics_month: Set(Some(c.topics_month)),
        topics_year: Set(Some(c.topics_year)),
        topics_all_time: Set(Some(c.topics_all_time)),
        parent_category_id: Set(c.parent_id),
        position: Set(Some(c.position)),
        read_restricted: Set(c.read_restricted),
        ..Default::default()
    };
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
                discourse_categories::Column::Description,
                discourse_categories::Column::DescriptionText,
                discourse_categories::Column::TopicsDay,
                discourse_categories::Column::TopicsWeek,
                discourse_categories::Column::TopicsMonth,
                discourse_categories::Column::TopicsYear,
                discourse_categories::Column::TopicsAllTime,
                discourse_categories::Column::ParentCategoryId,
                discourse_categories::Column::Position,
                discourse_categories::Column::ReadRestricted,
            ])
            .to_owned(),
        )
        .do_nothing()
        .exec(db)
        .await?;
    debug!("upserted category record");
    Ok(())
}
