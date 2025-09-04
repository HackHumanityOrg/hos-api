use crate::domain::likes::model::PostLikeResponse;
use crate::domain::users::model::UserProfile;
use crate::domain::users::repo::upsert_user_profiles_bulk;
use crate::domain::util::with_deadlock_retry;
use anyhow::Result;
use chrono::Utc;
use entities::discourse_post_likes;
use sea_orm::sea_query::OnConflict as OrmOnConflict;
use sea_orm::{ActiveValue::Set, EntityTrait};
use tracing::{debug, instrument};

#[instrument(skip(db, likes), fields(instance = instance, post_id = post_id))]
pub async fn upsert_post_likes_batch(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    post_id: i32,
    likes: &PostLikeResponse,
) -> Result<()> {
    if likes.post_action_users.is_empty() {
        return Ok(());
    }
    debug!(
        count = likes.post_action_users.len(),
        "upserting post likes batch"
    );
    // Ensure referenced users exist for FK (instance, external_user_id) -> users
    {
        let mut profiles: Vec<UserProfile> = likes
            .post_action_users
            .iter()
            .map(|u| UserProfile {
                id: u.id,
                username: u.username.clone(),
                name: None,
                avatar_template: String::new(),
                title: None,
            })
            .collect();
        // Deduplicate by id while keeping deterministic order
        profiles.sort_by_key(|p| p.id);
        profiles.dedup_by_key(|p| p.id);
        upsert_user_profiles_bulk(db, instance, &profiles).await?;
    }
    let now = Utc::now();
    // Normalize order to reduce deadlocks and chunk the writes
    let mut users = likes.post_action_users.clone();
    users.sort_by_key(|u| u.id);
    const CHUNK_SIZE: usize = 200;
    for chunk in users.chunks(CHUNK_SIZE) {
        let ams: Vec<discourse_post_likes::ActiveModel> = chunk
            .iter()
            .map(|u| discourse_post_likes::ActiveModel {
                instance: Set(instance.to_string()),
                external_discourse_post_id: Set(post_id),
                external_user_id: Set(u.id),
                username: Set(u.username.clone()),
                created_at: Set(now),
                ..Default::default()
            })
            .collect();
        with_deadlock_retry(|| async {
            discourse_post_likes::Entity::insert_many(ams.clone())
                .on_conflict(
                    OrmOnConflict::columns([
                        discourse_post_likes::Column::Instance,
                        discourse_post_likes::Column::ExternalDiscoursePostId,
                        discourse_post_likes::Column::ExternalUserId,
                    ])
                    .update_columns([
                        discourse_post_likes::Column::Username,
                        discourse_post_likes::Column::CreatedAt,
                    ])
                    .to_owned(),
                )
                .exec(db)
                .await
        })
        .await?;
    }
    debug!("upserted likes batch");
    Ok(())
}
