use anyhow::Result;
use sea_orm::prelude::Expr;
use sea_orm::sea_query::OnConflict;
use sea_orm::{ActiveValue::NotSet, ConnectionTrait, DatabaseConnection, EntityTrait, Set};
use std::collections::{BTreeSet, HashMap};
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};
use tracing::{debug, info};

/// Global in-memory cursor cache
static CURSORS: OnceLock<RwLock<HashMap<String, i64>>> = OnceLock::new();

/// Global rate limiter for DB writes
static LAST_WRITES: OnceLock<RwLock<HashMap<String, Instant>>> = OnceLock::new();

/// Heights processed ahead of the contiguous cursor per module.
/// We only advance persisted cursor when all intermediate heights are complete.
static PENDING_HEIGHTS: OnceLock<RwLock<HashMap<String, BTreeSet<i64>>>> = OnceLock::new();

fn get_cursors() -> &'static RwLock<HashMap<String, i64>> {
    CURSORS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_last_writes() -> &'static RwLock<HashMap<String, Instant>> {
    LAST_WRITES.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_pending_heights() -> &'static RwLock<HashMap<String, BTreeSet<i64>>> {
    PENDING_HEIGHTS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn advance_contiguous_cursor(
    cursor_map: &mut HashMap<String, i64>,
    pending_map: &mut HashMap<String, BTreeSet<i64>>,
    module_name: &str,
    height: i64,
) -> Option<i64> {
    let current = cursor_map.get(module_name).copied().unwrap_or(-1);

    if height <= current {
        return None;
    }

    let pending = pending_map.entry(module_name.to_string()).or_default();
    pending.insert(height);

    let mut next = current.saturating_add(1);
    while pending.remove(&next) {
        next = next.saturating_add(1);
    }

    let new_current = next.saturating_sub(1);
    if new_current > current {
        cursor_map.insert(module_name.to_string(), new_current);
        Some(new_current)
    } else {
        None
    }
}

/// Seed an in-memory cursor for modules without persisted cursor.
/// This is used to support configured non-zero start heights while preserving
/// contiguous cursor advancement under out-of-order processing.
pub fn seed_module_cursor(module_name: &str, height: i64) {
    let cursors = get_cursors();
    let mut cursor_map = cursors.write().unwrap();
    cursor_map.entry(module_name.to_string()).or_insert(height);

    debug!(
        module = module_name,
        height = height,
        "Seeded in-memory cursor"
    );
}

/// Initialize cursors from database
pub async fn initialize_cursor_storage(db: &DatabaseConnection) -> Result<()> {
    info!("Initializing cursor storage from database");

    // Reset pending non-contiguous state for fresh startup.
    get_pending_heights().write().unwrap().clear();

    // Load all cursors from database
    let all_cursors = entities::near::near_sync_cursors::Entity::find()
        .all(db)
        .await?;

    let cursors = get_cursors();
    let mut cursor_map = cursors.write().unwrap();
    for cursor_row in &all_cursors {
        let module_name = cursor_row
            .module_name
            .strip_prefix("near-indexer:")
            .unwrap_or(&cursor_row.module_name);
        cursor_map.insert(module_name.to_string(), cursor_row.last_processed_height);

        debug!(
            module = module_name,
            height = cursor_row.last_processed_height,
            "Loaded cursor from database"
        );
    }

    info!(
        cursor_count = all_cursors.len(),
        "Cursor storage initialized successfully"
    );
    Ok(())
}

/// Get the cursor for a specific module (from memory)
pub async fn get_module_cursor(_db: &DatabaseConnection, module_name: &str) -> Result<Option<i64>> {
    let cursors = get_cursors();
    let cursor_map = cursors.read().unwrap();
    let cursor_value = cursor_map.get(module_name).copied();

    debug!(
        module = module_name,
        cursor = cursor_value,
        "Retrieved cursor from cache"
    );

    Ok(cursor_value)
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
    let pending_heights = get_pending_heights();

    // Advance in-memory cursor only when contiguous heights are complete.
    let committed_height = {
        let mut cursor_map = cursors.write().unwrap();
        let mut pending_map = pending_heights.write().unwrap();
        advance_contiguous_cursor(&mut cursor_map, &mut pending_map, module_name, height)
    };

    let Some(committed_height) = committed_height else {
        debug!(
            module = module_name,
            processed_height = height,
            "Processed height did not advance contiguous cursor"
        );
        return Ok(());
    };

    debug!(
        module = module_name,
        processed_height = height,
        committed_height = committed_height,
        "Advanced contiguous cursor in memory cache"
    );

    // Check if we should write to DB (rate limited)
    let now = Instant::now();
    let should_write = {
        let writes = last_writes.read().unwrap();
        writes
            .get(module_name)
            .is_none_or(|&last| now.duration_since(last) >= Duration::from_secs(1))
    };

    if !should_write {
        debug!(
            module = module_name,
            height = committed_height,
            "Skipping database write due to rate limiting"
        );
        return Ok(());
    }

    // Update last write time
    {
        let mut writes = last_writes.write().unwrap();
        writes.insert(module_name.to_string(), now);
    }

    // Write to database
    let stream_name = format!("near-indexer:{}", module_name);
    entities::near::near_sync_cursors::Entity::insert(
        entities::near::near_sync_cursors::ActiveModel {
            module_name: Set(stream_name),
            last_processed_height: Set(committed_height),
            updated_at: NotSet,
        },
    )
    .on_conflict(
        OnConflict::column(entities::near::near_sync_cursors::Column::ModuleName)
            .update_columns([
                entities::near::near_sync_cursors::Column::LastProcessedHeight,
                entities::near::near_sync_cursors::Column::UpdatedAt,
            ])
            .values([
                (
                    entities::near::near_sync_cursors::Column::LastProcessedHeight,
                    Expr::value(committed_height),
                ),
                (
                    entities::near::near_sync_cursors::Column::UpdatedAt,
                    Expr::current_timestamp().into(),
                ),
            ])
            .to_owned(),
    )
    .exec(db)
    .await?;

    debug!(
        module = module_name,
        height = committed_height,
        "Successfully wrote cursor to database"
    );

    Ok(())
}

pub fn reset_cursor_storage() {
    info!("Resetting cursor storage (clearing all caches)");

    let cursor_count = if let Some(cursors) = CURSORS.get() {
        let count = cursors.read().unwrap().len();
        cursors.write().unwrap().clear();
        count
    } else {
        0
    };

    let write_count = if let Some(last_writes) = LAST_WRITES.get() {
        let count = last_writes.read().unwrap().len();
        last_writes.write().unwrap().clear();
        count
    } else {
        0
    };

    let pending_count = if let Some(pending) = PENDING_HEIGHTS.get() {
        let count = pending.read().unwrap().len();
        pending.write().unwrap().clear();
        count
    } else {
        0
    };

    info!(
        cursor_count = cursor_count,
        write_count = write_count,
        pending_count = pending_count,
        "Cursor storage reset completed"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_contiguous_cursor_waits_for_gaps() {
        let mut cursors = HashMap::new();
        let mut pending = HashMap::new();
        cursors.insert("tokens".to_string(), 99);

        // Processed ahead of gap: should not advance yet
        assert_eq!(
            advance_contiguous_cursor(&mut cursors, &mut pending, "tokens", 101),
            None
        );
        assert_eq!(cursors.get("tokens"), Some(&99));

        // Gap filled: should jump to contiguous max (101)
        assert_eq!(
            advance_contiguous_cursor(&mut cursors, &mut pending, "tokens", 100),
            Some(101)
        );
        assert_eq!(cursors.get("tokens"), Some(&101));
    }

    #[test]
    fn advance_contiguous_cursor_supports_seeded_starts() {
        let mut cursors = HashMap::new();
        let mut pending = HashMap::new();
        cursors.insert("staking_pools".to_string(), 499);

        assert_eq!(
            advance_contiguous_cursor(&mut cursors, &mut pending, "staking_pools", 501),
            None
        );
        assert_eq!(
            advance_contiguous_cursor(&mut cursors, &mut pending, "staking_pools", 500),
            Some(501)
        );
        assert_eq!(cursors.get("staking_pools"), Some(&501));
    }
}
