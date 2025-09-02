use crate::domain::users::model::DirectoryItem;
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
        .await?;
    debug!("upserted user");
    Ok(())
}
