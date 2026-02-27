use crate::domain::likes::model::PostActionUser;
use crate::domain::users::model::User;
use crate::domain::users::repo::upsert_users_bulk;
use anyhow::Result;
use chrono::{Timelike, Utc};
use entities::discourse::discourse_post_likes;
use sea_orm::sea_query::OnConflict as OrmOnConflict;
use sea_orm::{ActiveValue::Set, EntityTrait};
use tracing::{debug, instrument};

#[instrument(skip(db, likes), fields(instance = instance, post_id = post_id))]
pub async fn upsert_post_likes_batch(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    post_id: i32,
    likes: &[PostActionUser],
) -> Result<()> {
    if likes.is_empty() {
        return Ok(());
    }
    debug!(count = likes.len(), "upserting post likes batch");
    // Ensure referenced users exist for FK (instance, external_user_id) -> users
    {
        let mut profiles: Vec<User> = likes
            .iter()
            .map(|u| User {
                id: u.id,
                username: u.username.clone(),
                name: u.name.clone(),
                avatar_template: u.avatar_template.clone(),
                title: None,
                trust_level: 0,
                likes_received: None,
                likes_given: None,
                topics_entered: None,
                topic_count: None,
                post_count: None,
                posts_read: None,
                days_visited: None,
            })
            .collect();
        // Deduplicate by id while keeping deterministic order
        profiles.sort_by_key(|p| p.id);
        profiles.dedup_by_key(|p| p.id);
        upsert_users_bulk(db, instance, &profiles).await?;
    }
    // Column precision is seconds; ensure our value matches PostgreSQL storage semantics
    let now = Utc::now()
        .with_nanosecond(0)
        .expect("valid UTC timestamp")
        .naive_utc();
    // Normalize order for consistent writes and chunk the operations
    let mut users = likes.to_vec();
    users.sort_by_key(|u| u.id);
    const CHUNK_SIZE: usize = 200;
    for chunk in users.chunks(CHUNK_SIZE) {
        let ams: Vec<discourse_post_likes::ActiveModel> = chunk
            .iter()
            .map(|u| discourse_post_likes::ActiveModel {
                instance: Set(instance.to_string()),
                external_post_id: Set(post_id),
                external_user_id: Set(u.id),
                created_at: Set(now),
                ..Default::default()
            })
            .collect();
        discourse_post_likes::Entity::insert_many(ams.clone())
            .on_conflict(
                OrmOnConflict::columns([
                    discourse_post_likes::Column::Instance,
                    discourse_post_likes::Column::ExternalPostId,
                    discourse_post_likes::Column::ExternalUserId,
                ])
                .update_columns([discourse_post_likes::Column::CreatedAt])
                .to_owned(),
            )
            .exec(db)
            .await?;
    }
    debug!("upserted likes batch");
    Ok(())
}
