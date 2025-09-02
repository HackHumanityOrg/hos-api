use crate::domain::likes::model::PostLikeResponse;
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
    let now = Utc::now();
    let ams: Vec<discourse_post_likes::ActiveModel> = likes
        .post_action_users
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
    discourse_post_likes::Entity::insert_many(ams)
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
        .await?;
    debug!("upserted likes batch");
    Ok(())
}
