use crate::domain::users::model::{DirectoryItem, UserProfile};
use anyhow::Result;
use entities::discourse_users;
use rand::Rng;
use sea_orm::sea_query::OnConflict as OrmOnConflict;
use sea_orm::{ActiveValue::Set, EntityTrait};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use tracing::{debug, instrument};

// Per-instance async lock to serialize writes to `discourse_users` and avoid
// interleaving UPSERTs from directory, posts, and likes that can cause deadlocks.
static USER_LOCKS: OnceLock<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>> = OnceLock::new();

fn get_user_lock(instance: &str) -> Arc<tokio::sync::Mutex<()>> {
    let map_mutex = USER_LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut map = map_mutex.lock().unwrap();
    Arc::clone(
        map.entry(instance.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
    )
}

#[instrument(skip(db, item), fields(instance = instance, external_user_id = item.user.id))]
pub async fn upsert_user(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    item: &DirectoryItem,
) -> Result<()> {
    // Serialize per-instance writers to users
    let lock = get_user_lock(instance);
    let _guard = lock.lock().await;
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
    // Retry on deadlocks to avoid aborting batch runs under contention
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        let res = discourse_users::Entity::insert(am.clone())
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
                sea_orm::DbErr::RecordNotInserted => {
                    Ok(sea_orm::InsertResult { last_insert_id: 0 })
                }
                _ => Err(err),
            });
        match res {
            Ok(_) => break,
            Err(e) => {
                let s = e.to_string();
                let is_deadlock = s.contains("Deadlock found") || s.contains("errno 1213");
                if is_deadlock && attempts <= 5 {
                    let jitter_ms: u64 = rand::thread_rng().gen_range(0..20);
                    tokio::time::sleep(std::time::Duration::from_millis(
                        20 * attempts as u64 + jitter_ms,
                    ))
                    .await;
                    continue;
                }
                return Err(e.into());
            }
        }
    }
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
    // Serialize per-instance writers to users
    let lock = get_user_lock(instance);
    let _guard = lock.lock().await;
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

/// Bulk insert or update minimal user records based on `/u/{username}.json`-like data.
/// Metrics fields are set to 0 to satisfy NOT NULL constraints and can be
/// enriched later via the directory endpoints.
#[instrument(skip(db, profiles), fields(instance = instance, count = profiles.len()))]
pub async fn upsert_user_profiles_bulk(
    db: &sea_orm::DatabaseConnection,
    instance: &str,
    profiles: &[UserProfile],
) -> Result<()> {
    // Serialize per-instance writers to users
    let lock = get_user_lock(instance);
    let _guard = lock.lock().await;
    use rand::{thread_rng, Rng};
    if profiles.is_empty() {
        return Ok(());
    }
    // Deduplicate by external id to minimize conflicting updates
    let mut uniq = profiles.to_vec();
    uniq.sort_by_key(|p| p.id);
    uniq.dedup_by_key(|p| p.id);

    // Chunk to reduce lock footprint and the probability of deadlocks
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
                likes_received: Set(0),
                likes_given: Set(0),
                topics_entered: Set(0),
                topic_count: Set(0),
                post_count: Set(0),
                posts_read: Set(0),
                days_visited: Set(0),
                ..Default::default()
            })
            .collect();

        // Retry on deadlock with small jittered backoff
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            let res = discourse_users::Entity::insert_many(ams.clone())
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
                    sea_orm::DbErr::RecordNotInserted => {
                        Ok(sea_orm::InsertResult { last_insert_id: 0 })
                    }
                    _ => Err(err),
                });
            match res {
                Ok(_) => break,
                Err(e) => {
                    let s = e.to_string();
                    let is_deadlock = s.contains("Deadlock found") || s.contains("errno 1213");
                    if is_deadlock && attempts <= 5 {
                        let sleep_ms: u64 = 20 * attempts as u64 + thread_rng().gen_range(0..20);
                        tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
                        continue;
                    }
                    return Err(e.into());
                }
            }
        }
    }
    debug!("upserted user profiles batch");
    Ok(())
}
