use crate::domain::posts::model::Post;
use crate::domain::users::model::UserProfile;
use crate::domain::users::repo::upsert_user_profiles_bulk;
use crate::domain::util::with_deadlock_retry;
use crate::domain::util::{bool_i8, opt_string};
use anyhow::Result;
use entities::discourse_indexer_posts;
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
        let mut uniq: HashMap<i32, UserProfile> = HashMap::new();
        for p in posts.iter() {
            uniq.entry(p.user_id).or_insert(UserProfile {
                id: p.user_id,
                username: p.username.clone(),
                name: p.name.clone(),
                avatar_template: p.avatar_template.clone(),
                title: None,
            });
        }
        let mut profiles: Vec<UserProfile> = uniq.into_values().collect();
        // Keep deterministic order for easier debugging
        profiles.sort_by_key(|u| u.id);
        upsert_user_profiles_bulk(db, instance, &profiles).await?;
    }
    // Normalize insert order and chunk to reduce lock contention
    let mut sorted_posts = posts.to_vec();
    sorted_posts.sort_by_key(|p| p.id);
    const CHUNK_SIZE: usize = 200;
    for chunk in sorted_posts.chunks(CHUNK_SIZE) {
        let ams: Vec<discourse_indexer_posts::ActiveModel> = chunk
            .iter()
            .map(|p| discourse_indexer_posts::ActiveModel {
                instance: Set(instance.to_string()),
                external_id: Set(p.id),
                name: Set(opt_string(p.name.clone())),
                username: Set(p.username.clone()),
                avatar_template: Set(p.avatar_template.clone()),
                created_at: Set(p.created_at),
                cooked: Set(p.cooked.clone()),
                post_number: Set(p.post_number),
                post_type: Set(p.post_type),
                updated_at: Set(p.updated_at),
                reply_count: Set(p.reply_count),
                reply_to_post_number: Set(p.reply_to_post_number),
                quote_count: Set(p.quote_count),
                incoming_link_count: Set(p.incoming_link_count),
                reads: Set(p.reads),
                readers_count: Set(p.readers_count),
                score: Set(p.score),
                topic_id: Set(p.topic_id),
                topic_slug: Set(p.topic_slug.clone()),
                display_username: Set(opt_string(p.display_username.clone())),
                primary_group_name: Set(p.primary_group_name.clone()),
                flair_name: Set(p.flair_name.clone()),
                flair_url: Set(p.flair_url.clone()),
                flair_bg_color: Set(p.flair_bg_color.clone()),
                flair_color: Set(p.flair_color.clone()),
                version: Set(p.version),
                user_title: Set(p.user_title.clone()),
                moderator: Set(bool_i8(p.moderator)),
                admin: Set(bool_i8(p.admin)),
                staff: Set(bool_i8(p.staff)),
                user_id: Set(p.user_id),
                hidden: Set(bool_i8(p.hidden)),
                trust_level: Set(p.trust_level),
                deleted_at: Set(p.deleted_at.as_ref().and_then(|s| s.parse().ok())),
                user_deleted: Set(bool_i8(p.user_deleted)),
                edit_reason: Set(p.edit_reason.clone()),
                wiki: Set(bool_i8(p.wiki)),
                raw: Set(p.raw.clone()),
                ..Default::default()
            })
            .collect();
        with_deadlock_retry(|| async {
            discourse_indexer_posts::Entity::insert_many(ams.clone())
                .on_conflict(
                    OrmOnConflict::columns([
                        discourse_indexer_posts::Column::Instance,
                        discourse_indexer_posts::Column::ExternalId,
                    ])
                    .update_columns([
                        discourse_indexer_posts::Column::Name,
                        discourse_indexer_posts::Column::Username,
                        discourse_indexer_posts::Column::AvatarTemplate,
                        discourse_indexer_posts::Column::CreatedAt,
                        discourse_indexer_posts::Column::Cooked,
                        discourse_indexer_posts::Column::PostNumber,
                        discourse_indexer_posts::Column::PostType,
                        discourse_indexer_posts::Column::UpdatedAt,
                        discourse_indexer_posts::Column::ReplyCount,
                        discourse_indexer_posts::Column::ReplyToPostNumber,
                        discourse_indexer_posts::Column::QuoteCount,
                        discourse_indexer_posts::Column::IncomingLinkCount,
                        discourse_indexer_posts::Column::Reads,
                        discourse_indexer_posts::Column::ReadersCount,
                        discourse_indexer_posts::Column::Score,
                        discourse_indexer_posts::Column::TopicId,
                        discourse_indexer_posts::Column::TopicSlug,
                        discourse_indexer_posts::Column::DisplayUsername,
                        discourse_indexer_posts::Column::PrimaryGroupName,
                        discourse_indexer_posts::Column::FlairName,
                        discourse_indexer_posts::Column::FlairUrl,
                        discourse_indexer_posts::Column::FlairBgColor,
                        discourse_indexer_posts::Column::FlairColor,
                        discourse_indexer_posts::Column::Version,
                        discourse_indexer_posts::Column::UserTitle,
                        discourse_indexer_posts::Column::Moderator,
                        discourse_indexer_posts::Column::Admin,
                        discourse_indexer_posts::Column::Staff,
                        discourse_indexer_posts::Column::UserId,
                        discourse_indexer_posts::Column::Hidden,
                        discourse_indexer_posts::Column::TrustLevel,
                        discourse_indexer_posts::Column::DeletedAt,
                        discourse_indexer_posts::Column::UserDeleted,
                        discourse_indexer_posts::Column::EditReason,
                        discourse_indexer_posts::Column::Wiki,
                        discourse_indexer_posts::Column::Raw,
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
    }
    debug!("upserted posts batch");
    Ok(())
}
