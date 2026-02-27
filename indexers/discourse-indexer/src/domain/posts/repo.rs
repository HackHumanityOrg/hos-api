use crate::domain::posts::model::Post;
use crate::domain::users::model::User;
use crate::domain::users::repo::upsert_users_bulk;
use anyhow::Result;
use chrono::Timelike;
use entities::discourse::discourse_posts;
use sea_orm::sea_query::OnConflict as OrmOnConflict;
use sea_orm::{ActiveValue::Set, EntityTrait};
use tracing::{debug, instrument};

#[instrument(skip(db, posts), fields(instance = instance))]
pub async fn upsert_posts_bulk(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    posts: &[Post],
) -> Result<()> {
    if posts.is_empty() {
        return Ok(());
    }
    debug!(count = posts.len(), "upserting posts bulk");

    // Ensure referenced users exist (including Discourse's system user id -1)
    // Build a unique set of minimal user profiles from post payloads.
    {
        use std::collections::HashMap;
        let mut uniq: HashMap<i32, User> = HashMap::new();
        for p in posts.iter() {
            uniq.entry(p.user_id).or_insert(User {
                id: p.user_id,
                username: p.username.clone(),
                name: p.name.clone(),
                avatar_template: p.avatar_template.clone(),
                title: None,
                trust_level: 0,
                likes_received: None,
                likes_given: None,
                topics_entered: None,
                topic_count: None,
                post_count: None,
                posts_read: None,
                days_visited: None,
            });
        }
        let mut profiles: Vec<User> = uniq.into_values().collect();
        // Keep deterministic order for easier debugging
        profiles.sort_by_key(|u| u.id);
        upsert_users_bulk(db, instance, &profiles).await?;
    }
    // Normalize insert order and chunk to reduce lock contention
    let mut sorted_posts = posts.to_vec();
    sorted_posts.sort_by_key(|p| p.id);
    const CHUNK_SIZE: usize = 200;
    for chunk in sorted_posts.chunks(CHUNK_SIZE) {
        let ams: Vec<discourse_posts::ActiveModel> = chunk
            .iter()
            .map(|p| discourse_posts::ActiveModel {
                created_at: Set(p
                    .created_at
                    .with_nanosecond(0)
                    .expect("valid created_at timestamp")
                    .naive_utc()),
                updated_at: Set(p
                    .updated_at
                    .with_nanosecond(0)
                    .expect("valid updated_at timestamp")
                    .naive_utc()),
                instance: Set(instance.to_string()),
                external_id: Set(p.id),
                name: Set(p.name.clone()),
                username: Set(p.username.clone()),
                post_number: Set(p.post_number),
                post_type: Set(p.post_type),
                raw: Set(p.raw.clone()),
                reply_count: Set(p.reply_count),
                reply_to_post_number: Set(p.reply_to_post_number),
                quote_count: Set(p.quote_count),
                incoming_link_count: Set(p.incoming_link_count),
                reads: Set(p.reads),
                readers_count: Set(p.readers_count),
                score: Set(p.score),
                external_topic_id: Set(p.topic_id),
                topic_slug: Set(p.topic_slug.clone()),
                display_username: Set(p.display_username.clone()),
                primary_group_name: Set(p.primary_group_name.clone()),
                flair_name: Set(p.flair_name.clone()),
                flair_url: Set(p.flair_url.clone()),
                flair_bg_color: Set(p.flair_bg_color.clone()),
                flair_color: Set(p.flair_color.clone()),
                version: Set(p.version),
                external_user_id: Set(p.user_id),
                can_view_edit_history: Set(p.can_view_edit_history),
                deleted: Set(p.deleted),
                ..Default::default()
            })
            .collect();
        discourse_posts::Entity::insert_many(ams.clone())
            .on_conflict(
                OrmOnConflict::columns([
                    discourse_posts::Column::Instance,
                    discourse_posts::Column::ExternalId,
                ])
                .update_columns([
                    discourse_posts::Column::Name,
                    discourse_posts::Column::Username,
                    discourse_posts::Column::CreatedAt,
                    discourse_posts::Column::Raw,
                    discourse_posts::Column::PostNumber,
                    discourse_posts::Column::PostType,
                    discourse_posts::Column::UpdatedAt,
                    discourse_posts::Column::ReplyCount,
                    discourse_posts::Column::ReplyToPostNumber,
                    discourse_posts::Column::QuoteCount,
                    discourse_posts::Column::IncomingLinkCount,
                    discourse_posts::Column::Reads,
                    discourse_posts::Column::ReadersCount,
                    discourse_posts::Column::Score,
                    discourse_posts::Column::ExternalTopicId,
                    discourse_posts::Column::TopicSlug,
                    discourse_posts::Column::DisplayUsername,
                    discourse_posts::Column::PrimaryGroupName,
                    discourse_posts::Column::FlairName,
                    discourse_posts::Column::FlairUrl,
                    discourse_posts::Column::FlairBgColor,
                    discourse_posts::Column::FlairColor,
                    discourse_posts::Column::Version,
                    discourse_posts::Column::ExternalUserId,
                    discourse_posts::Column::CanViewEditHistory,
                    discourse_posts::Column::Deleted,
                ])
                .to_owned(),
            )
            .do_nothing()
            .exec(db)
            .await?;
    }
    debug!("upserted posts batch");
    Ok(())
}
