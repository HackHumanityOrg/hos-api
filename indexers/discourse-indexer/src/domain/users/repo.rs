use crate::domain::users::model::{DirectoryItem, UserProfile};
use anyhow::Result;
use entities::discourse_users;
use sea_orm::sea_query::OnConflict as OrmOnConflict;
use sea_orm::{ActiveValue::Set, EntityTrait};
use tracing::{debug, instrument};

#[instrument(skip(db, item), fields(instance = instance, external_user_id = item.user.id))]
pub async fn upsert_user(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    item: &DirectoryItem,
) -> Result<()> {
    let u = &item.user;

    let am = discourse_users::ActiveModel {
        instance: Set(instance.to_string()),
        external_id: Set(u.id),
        username: Set(u.username.clone()),
        name: Set(u.name.clone()),
        avatar_template: Set(u.avatar_template.clone()),
        title: Set(u.title.clone()),
        likes_received: Set(item.likes_received),
        likes_given: Set(item.likes_given),
        topics_entered: Set(item.topics_entered),
        topic_count: Set(item.topic_count),
        post_count: Set(item.post_count),
        posts_read: Set(item.posts_read),
        days_visited: Set(item.days_visited),
        ..Default::default()
    };
    discourse_users::Entity::insert(am)
        .on_conflict(
            OrmOnConflict::columns([
                discourse_users::Column::Instance,
                discourse_users::Column::ExternalId,
            ])
            .update_columns([
                discourse_users::Column::Username,
                discourse_users::Column::Name,
                discourse_users::Column::AvatarTemplate,
                discourse_users::Column::Title,
                discourse_users::Column::LikesReceived,
                discourse_users::Column::LikesGiven,
                discourse_users::Column::TopicsEntered,
                discourse_users::Column::TopicCount,
                discourse_users::Column::PostCount,
                discourse_users::Column::PostsRead,
                discourse_users::Column::DaysVisited,
            ])
            .to_owned(),
        )
        .exec(db)
        .await
        .or_else(|err| match err {
            // MySQL may report this when ON CONFLICT results in an update instead of an insert
            sea_orm::DbErr::RecordNotInserted => Ok(sea_orm::InsertResult { last_insert_id: 0 }),
            _ => Err(err),
        })?;
    debug!("upserted user");
    Ok(())
}

/// Insert or update a minimal user record based on `/u/{username}.json` data.
/// Metrics fields are set to 0 to satisfy NOT NULL constraints and can be
/// enriched later via the directory endpoints.
#[instrument(skip(db, profile), fields(instance = instance, external_user_id = profile.id))]
pub async fn upsert_user_profile(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    profile: &UserProfile,
) -> Result<()> {
    let am = discourse_users::ActiveModel {
        instance: Set(instance.to_string()),
        external_id: Set(profile.id),
        username: Set(profile.username.clone()),
        name: Set(profile.name.clone()),
        avatar_template: Set(profile.avatar_template.clone()),
        title: Set(profile.title.clone()),
        likes_received: Set(0),
        likes_given: Set(0),
        topics_entered: Set(0),
        topic_count: Set(0),
        post_count: Set(0),
        posts_read: Set(0),
        days_visited: Set(0),
        ..Default::default()
    };
    discourse_users::Entity::insert(am)
        .on_conflict(
            OrmOnConflict::columns([
                discourse_users::Column::Instance,
                discourse_users::Column::ExternalId,
            ])
            .update_columns([
                discourse_users::Column::Username,
                discourse_users::Column::Name,
                discourse_users::Column::AvatarTemplate,
                discourse_users::Column::Title,
            ])
            .to_owned(),
        )
        .exec(db)
        .await
        .or_else(|err| match err {
            sea_orm::DbErr::RecordNotInserted => Ok(sea_orm::InsertResult { last_insert_id: 0 }),
            _ => Err(err),
        })?;
    debug!("upserted user profile");
    Ok(())
}
