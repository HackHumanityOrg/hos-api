use crate::domain::topics::model::Topic;
use crate::domain::util::{opt_bool_i8, opt_string};
use anyhow::Result;
use entities::discourse_topics;
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
    let ams: Vec<discourse_topics::ActiveModel> = topics
        .iter()
        .map(|t| discourse_topics::ActiveModel {
            instance: Set(instance.to_string()),
            external_id: Set(t.id),
            title: Set(t.title.clone()),
            fancy_title: Set(t.fancy_title.clone()),
            slug: Set(t.slug.clone()),
            posts_count: Set(t.posts_count),
            reply_count: Set(t.reply_count),
            highest_post_number: Set(t.highest_post_number),
            image_url: Set(opt_string(t.image_url.clone())),
            created_at: Set(t.created_at),
            last_posted_at: Set(t.last_posted_at),
            bumped: Set(opt_bool_i8(t.bumped, 0)),
            bumped_at: Set(t.bumped_at),
            archetype: Set(t.archetype.clone()),
            pinned: Set(opt_bool_i8(t.pinned, 0)),
            unpinned: Set(t.unpinned.clone()),
            visible: Set(opt_bool_i8(t.visible, 1)),
            closed: Set(opt_bool_i8(t.closed, 0)),
            archived: Set(opt_bool_i8(t.archived, 0)),
            notification_level: Set(t.notification_level),
            views: Set(t.views),
            like_count: Set(t.like_count),
            has_summary: Set(opt_bool_i8(t.has_summary, 0)),
            last_poster_username: Set(opt_string(t.last_poster_username.clone())),
            category_id: Set(t.category_id),
            op_like_count: Set(t.op_like_count.unwrap_or(0)),
            pinned_globally: Set(opt_bool_i8(t.pinned_globally, 0)),
            featured_link: Set(t.featured_link.clone()),
            ..Default::default()
        })
        .collect();
    discourse_topics::Entity::insert_many(ams)
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
                discourse_topics::Column::HighestPostNumber,
                discourse_topics::Column::ImageUrl,
                discourse_topics::Column::CreatedAt,
                discourse_topics::Column::LastPostedAt,
                discourse_topics::Column::Bumped,
                discourse_topics::Column::BumpedAt,
                discourse_topics::Column::Archetype,
                discourse_topics::Column::Pinned,
                discourse_topics::Column::Unpinned,
                discourse_topics::Column::Visible,
                discourse_topics::Column::Closed,
                discourse_topics::Column::Archived,
                discourse_topics::Column::NotificationLevel,
                discourse_topics::Column::Views,
                discourse_topics::Column::LikeCount,
                discourse_topics::Column::HasSummary,
                discourse_topics::Column::LastPosterUsername,
                discourse_topics::Column::CategoryId,
                discourse_topics::Column::OpLikeCount,
                discourse_topics::Column::PinnedGlobally,
                discourse_topics::Column::FeaturedLink,
            ])
            .to_owned(),
        )
        .exec(db)
        .await
        .or_else(|err| match err {
            sea_orm::DbErr::RecordNotInserted => Ok(sea_orm::InsertResult { last_insert_id: 0 }),
            _ => Err(err),
        })?;
    debug!("upserted topics batch");
    Ok(())
}
