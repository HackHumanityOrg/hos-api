use crate::domain::users::model::{DirectoryItem, User, UserProfile};
use anyhow::Result;
use entities::discourse::discourse_users;
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
        likes_received: Set(item.likes_received.map(|v| v as i64)),
        likes_given: Set(item.likes_given.map(|v| v as i64)),
        topics_entered: Set(item.topics_entered.map(|v| v as i64)),
        topic_count: Set(item.topic_count.map(|v| v as i64)),
        post_count: Set(item.post_count.map(|v| v as i64)),
        posts_read: Set(item.posts_read.map(|v| v as i64)),
        days_visited: Set(item.days_visited.map(|v| v as i64)),
        trust_level: Set(Some(u.trust_level)),
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
                discourse_users::Column::TrustLevel,
            ])
            .to_owned(),
        )
        .do_nothing()
        .exec(db)
        .await?;
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
        likes_received: Set(None),
        likes_given: Set(None),
        topics_entered: Set(None),
        topic_count: Set(None),
        post_count: Set(None),
        posts_read: Set(None),
        days_visited: Set(None),
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
        .do_nothing()
        .exec(db)
        .await?;
    debug!("upserted user profile");
    Ok(())
}

/// Bulk insert or update minimal user records based on `/u/{username}.json`-like data.
/// Metrics fields are set to 0 to satisfy NOT NULL constraints and can be
/// enriched later via the directory endpoints.
#[instrument(skip(db, profiles), fields(instance = instance, count = profiles.len()))]
pub async fn upsert_user_profiles_bulk(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    profiles: &[UserProfile],
) -> Result<()> {
    if profiles.is_empty() {
        return Ok(());
    }
    // Deduplicate by external id to minimize conflicting updates
    let mut uniq = profiles.to_vec();
    uniq.sort_by_key(|p| p.id);
    uniq.dedup_by_key(|p| p.id);

    // Chunk to reduce large batch sizes into manageable operations
    const CHUNK_SIZE: usize = 200;
    for chunk in uniq.chunks(CHUNK_SIZE) {
        let ams: Vec<discourse_users::ActiveModel> = chunk
            .iter()
            .map(|p| discourse_users::ActiveModel {
                instance: Set(instance.to_string()),
                external_id: Set(p.id),
                username: Set(p.username.clone()),
                name: Set(p.name.clone()),
                avatar_template: Set(p.avatar_template.clone()),
                title: Set(p.title.clone()),
                likes_received: Set(None),
                likes_given: Set(None),
                topics_entered: Set(None),
                topic_count: Set(None),
                post_count: Set(None),
                posts_read: Set(None),
                days_visited: Set(None),
                ..Default::default()
            })
            .collect();

        discourse_users::Entity::insert_many(ams)
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
            .do_nothing()
            .exec(db)
            .await?;
    }
    debug!("upserted user profiles batch");
    Ok(())
}

/// Bulk upsert simplified User records (for foreign key maintenance)
#[instrument(skip(db, users), fields(instance = instance, count = users.len()))]
pub async fn upsert_users_bulk(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    users: &[User],
) -> Result<()> {
    if users.is_empty() {
        return Ok(());
    }

    let mut uniq = users.to_vec();
    uniq.sort_by_key(|u| u.id);
    uniq.dedup_by_key(|u| u.id);

    const CHUNK_SIZE: usize = 200;
    for chunk in uniq.chunks(CHUNK_SIZE) {
        let ams: Vec<discourse_users::ActiveModel> = chunk
            .iter()
            .map(|u| discourse_users::ActiveModel {
                instance: Set(instance.to_string()),
                external_id: Set(u.id),
                username: Set(u.username.clone()),
                name: Set(u.name.clone()),
                avatar_template: Set(u.avatar_template.clone()),
                title: Set(u.title.clone()),
                likes_received: Set(None),
                likes_given: Set(None),
                topics_entered: Set(None),
                topic_count: Set(None),
                post_count: Set(None),
                posts_read: Set(None),
                days_visited: Set(None),
                ..Default::default()
            })
            .collect();

        discourse_users::Entity::insert_many(ams)
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
            .do_nothing()
            .exec(db)
            .await?;
    }
    debug!("upserted simple users batch");
    Ok(())
}
