use crate::domain::topics::model::Topic;
use anyhow::Result;
use db_core::with_db_retry;
use entities::discourse::discourse_topics;
use sea_orm::sea_query::OnConflict as OrmOnConflict;
use sea_orm::{ActiveValue::Set, EntityTrait};
use tracing::{debug, instrument};

#[instrument(skip(db, topics), fields(instance = instance))]
pub async fn upsert_topics_bulk(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    topics: &[Topic],
) -> Result<()> {
    if topics.is_empty() {
        return Ok(());
    }
    debug!(count = topics.len(), "upserting topics bulk");
    // Normalize order for consistent lock acquisition and chunk to reduce lock footprint
    let mut sorted = topics.to_vec();
    sorted.sort_by_key(|t| t.id);
    const CHUNK_SIZE: usize = 200;
    for chunk in sorted.chunks(CHUNK_SIZE) {
        let ams: Vec<discourse_topics::ActiveModel> = chunk
            .iter()
            .map(|t| discourse_topics::ActiveModel {
                instance: Set(instance.to_string()),
                external_id: Set(t.id),
                title: Set(t.title.clone()),
                fancy_title: Set(t.fancy_title.clone().unwrap_or_else(|| t.title.clone())),
                slug: Set(t.slug.clone()),
                posts_count: Set(t.posts_count),
                reply_count: Set(t.reply_count),
                created_at: Set(t.created_at.naive_utc()),
                last_posted_at: Set(t.last_posted_at.naive_utc()),
                bumped_at: Set(t.bumped_at.naive_utc()),
                pinned: Set(t.pinned.unwrap_or(false)),
                pinned_globally: Set(t.pinned_globally.unwrap_or(false)),
                visible: Set(t.visible.unwrap_or(true)),
                closed: Set(t.closed.unwrap_or(false)),
                archived: Set(t.archived.unwrap_or(false)),
                views: Set(t.views),
                like_count: Set(t.like_count),
                external_category_id: Set(t.category_id),
                ..Default::default()
            })
            .collect();
        with_db_retry(|| async {
            discourse_topics::Entity::insert_many(ams.clone())
                .on_conflict(
                    OrmOnConflict::columns([
                        discourse_topics::Column::Instance,
                        discourse_topics::Column::ExternalId,
                    ])
                    .update_columns([
                        discourse_topics::Column::Title,
                        discourse_topics::Column::FancyTitle,
                        discourse_topics::Column::Slug,
                        discourse_topics::Column::PostsCount,
                        discourse_topics::Column::ReplyCount,
                        discourse_topics::Column::CreatedAt,
                        discourse_topics::Column::LastPostedAt,
                        discourse_topics::Column::BumpedAt,
                        discourse_topics::Column::Pinned,
                        discourse_topics::Column::PinnedGlobally,
                        discourse_topics::Column::Visible,
                        discourse_topics::Column::Closed,
                        discourse_topics::Column::Archived,
                        discourse_topics::Column::Views,
                        discourse_topics::Column::LikeCount,
                        discourse_topics::Column::ExternalCategoryId,
                    ])
                    .to_owned(),
                )
                .do_nothing()
                .exec(db)
                .await
        })
        .await?;
    }
    debug!("upserted topics batch");
    Ok(())
}
