use anyhow::Result;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveValue::NotSet, ConnectionTrait, DatabaseConnection, EntityTrait, Set};
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

/// Global in-memory cursor cache
static CURSORS: OnceLock<RwLock<HashMap<String, i64>>> = OnceLock::new();

/// Global rate limiter for DB writes
static LAST_WRITES: OnceLock<RwLock<HashMap<String, Instant>>> = OnceLock::new();

fn get_cursors() -> &'static RwLock<HashMap<String, i64>> {
    CURSORS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_last_writes() -> &'static RwLock<HashMap<String, Instant>> {
    LAST_WRITES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Initialize cursors from database
pub async fn initialize_cursor_storage(db: &DatabaseConnection) -> Result<()> {
    // Load all cursors from database
    let all_cursors = entities::near_sync_cursors::Entity::find().all(db).await?;

    let cursors = get_cursors();
    let mut cursor_map = cursors.write().unwrap();
    for cursor_row in all_cursors {
        let module_name = cursor_row
            .module_name
            .strip_prefix("near-indexer:")
            .unwrap_or(&cursor_row.module_name);
        cursor_map.insert(module_name.to_string(), cursor_row.last_processed_height);
    }

    Ok(())
}

/// Get the cursor for a specific module (from memory)
pub async fn get_module_cursor(_db: &DatabaseConnection, module_name: &str) -> Result<Option<i64>> {
    let cursors = get_cursors();
    let cursor_map = cursors.read().unwrap();
    Ok(cursor_map.get(module_name).copied())
}

/// Determine the start height for a module using stored cursor or configured start
pub async fn choose_module_start_height(
    db: &DatabaseConnection,
    module_name: &str,
    configured_start: u64,
) -> Result<(u64, bool)> {
    let cursor = get_module_cursor(db, module_name).await?;
    if let Some(last_height) = cursor {
        let last = if last_height < 0 {
            0u64
        } else {
            last_height as u64
        };
        let next = last.saturating_add(1);
        let chosen = std::cmp::max(configured_start, next);
        Ok((chosen, true))
    } else {
        Ok((configured_start, false))
    }
}

/// Update the cursor for a specific module (rate limited to max once per second per cursor)
pub async fn upsert_module_cursor<C>(db: &C, module_name: &str, height: i64) -> Result<()>
where
    C: ConnectionTrait,
{
    let cursors = get_cursors();
    let last_writes = get_last_writes();

    // Update in memory
    {
        let mut cursor_map = cursors.write().unwrap();
        if cursor_map
            .get(module_name)
            .is_none_or(|&current| height > current)
        {
            cursor_map.insert(module_name.to_string(), height);
        }
    }

    // Check if we should write to DB (rate limited)
    let now = Instant::now();
    let should_write = {
        let writes = last_writes.read().unwrap();
        writes
            .get(module_name)
            .is_none_or(|&last| now.duration_since(last) >= Duration::from_secs(1))
    };

    if !should_write {
        return Ok(());
    }

    // Update last write time
    {
        let mut writes = last_writes.write().unwrap();
        writes.insert(module_name.to_string(), now);
    }

    // Write to database
    let stream_name = format!("near-indexer:{}", module_name);
    entities::near_sync_cursors::Entity::insert(entities::near_sync_cursors::ActiveModel {
        module_name: Set(stream_name),
        last_processed_height: Set(height),
        updated_at: NotSet,
    })
    .on_conflict(
        OnConflict::column(entities::near_sync_cursors::Column::ModuleName)
            .update_columns([
                entities::near_sync_cursors::Column::LastProcessedHeight,
                entities::near_sync_cursors::Column::UpdatedAt,
            ])
            .values([
                (
                    entities::near_sync_cursors::Column::LastProcessedHeight,
                    Expr::value(height),
                ),
                (
                    entities::near_sync_cursors::Column::UpdatedAt,
                    Expr::current_timestamp().into(),
                ),
            ])
            .to_owned(),
    )
    .exec(db)
    .await?;

    Ok(())
}

pub fn reset_cursor_storage() {
    if let Some(cursors) = CURSORS.get() {
        cursors.write().unwrap().clear();
    }
    if let Some(last_writes) = LAST_WRITES.get() {
        last_writes.write().unwrap().clear();
    }
}
