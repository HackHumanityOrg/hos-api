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

/// Processed blocks that are waiting for their parent link per module.
#[derive(Default)]
struct PendingHeights {
    /// `height -> prev_height`
    by_height: HashMap<i64, i64>,
    /// `prev_height -> {child heights}`
    children_by_prev: HashMap<i64, BTreeSet<i64>>,
}

impl PendingHeights {
    fn insert(&mut self, height: i64, prev_height: i64) {
        if let Some(previous_prev_height) = self.by_height.insert(height, prev_height) {
            let should_drop_prev_entry =
                if let Some(children) = self.children_by_prev.get_mut(&previous_prev_height) {
                    children.remove(&height);
                    children.is_empty()
                } else {
                    false
                };

            if should_drop_prev_entry {
                self.children_by_prev.remove(&previous_prev_height);
            }
        }

        self.children_by_prev
            .entry(prev_height)
            .or_default()
            .insert(height);
    }

    fn pop_next_after(&mut self, prev_height: i64) -> Option<i64> {
        let next_height = self
            .children_by_prev
            .get(&prev_height)
            .and_then(|children| children.iter().next().copied());

        let next_height = next_height?;

        let should_drop_prev_entry =
            if let Some(children) = self.children_by_prev.get_mut(&prev_height) {
                children.remove(&next_height);
                children.is_empty()
            } else {
                false
            };

        if should_drop_prev_entry {
            self.children_by_prev.remove(&prev_height);
        }

        self.by_height.remove(&next_height);
        Some(next_height)
    }
}

/// Processed blocks pending cursor advancement per module.
/// Cursor advances only when a processed block has `prev_height` that links to
/// the current module cursor. This handles out-of-order processing and chains
/// with missing numeric heights.
static PENDING_HEIGHTS: OnceLock<RwLock<HashMap<String, PendingHeights>>> = OnceLock::new();

fn get_cursors() -> &'static RwLock<HashMap<String, i64>> {
    CURSORS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_last_writes() -> &'static RwLock<HashMap<String, Instant>> {
    LAST_WRITES.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_pending_heights() -> &'static RwLock<HashMap<String, PendingHeights>> {
    PENDING_HEIGHTS.get_or_init(|| RwLock::new(HashMap::new()))
}

fn advance_chain_cursor(
    cursor_map: &mut HashMap<String, i64>,
    pending_map: &mut HashMap<String, PendingHeights>,
    module_name: &str,
    height: i64,
    prev_height: Option<i64>,
) -> Option<i64> {
    let current = cursor_map.get(module_name).copied().unwrap_or(-1);

    if height <= current {
        return None;
    }

    // Some providers omit prev_height for genesis-style records.
    // Fall back to `height - 1` so cursor logic keeps moving.
    let prev_height = prev_height.unwrap_or_else(|| height.saturating_sub(1));

    let pending = pending_map.entry(module_name.to_string()).or_default();
    pending.insert(height, prev_height);

    let mut new_current = current;
    while let Some(next_height) = pending.pop_next_after(new_current) {
        new_current = next_height;
    }

    if new_current > current {
        cursor_map.insert(module_name.to_string(), new_current);
        Some(new_current)
    } else {
        None
    }
}

/// Seed an in-memory cursor for modules without persisted cursor.
/// This is used to support configured non-zero start heights while preserving
/// chain-linked cursor advancement under out-of-order processing.
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

    // Reset pending chain-link state for fresh startup.
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
pub async fn upsert_module_cursor<C>(
    db: &C,
    module_name: &str,
    height: i64,
    prev_height: Option<i64>,
) -> Result<()>
where
    C: ConnectionTrait,
{
    let cursors = get_cursors();
    let last_writes = get_last_writes();
    let pending_heights = get_pending_heights();

    // Advance in-memory cursor only when parent-linked blocks are complete.
    let committed_height = {
        let mut cursor_map = cursors.write().unwrap();
        let mut pending_map = pending_heights.write().unwrap();
        advance_chain_cursor(
            &mut cursor_map,
            &mut pending_map,
            module_name,
            height,
            prev_height,
        )
    };

    let Some(committed_height) = committed_height else {
        debug!(
            module = module_name,
            processed_height = height,
            processed_prev_height = prev_height,
            "Processed block did not advance chain-linked cursor"
        );
        return Ok(());
    };

    debug!(
        module = module_name,
        processed_height = height,
        processed_prev_height = prev_height,
        committed_height = committed_height,
        "Advanced chain-linked cursor in memory cache"
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
            .update_column(entities::near::near_sync_cursors::Column::LastProcessedHeight)
            .value(
                entities::near::near_sync_cursors::Column::UpdatedAt,
                Expr::current_timestamp(),
            )
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
    use sea_orm::{DbBackend, QueryTrait};

    #[test]
    fn advance_chain_cursor_waits_for_parent_link() {
        let mut cursors = HashMap::new();
        let mut pending = HashMap::new();
        cursors.insert("tokens".to_string(), 99);

        // Processed ahead of gap: should not advance yet
        assert_eq!(
            advance_chain_cursor(&mut cursors, &mut pending, "tokens", 101, Some(100)),
            None
        );
        assert_eq!(cursors.get("tokens"), Some(&99));

        // Parent now available: should advance to 101
        assert_eq!(
            advance_chain_cursor(&mut cursors, &mut pending, "tokens", 100, Some(99)),
            Some(101)
        );
        assert_eq!(cursors.get("tokens"), Some(&101));
    }

    #[test]
    fn advance_chain_cursor_handles_missing_numeric_heights() {
        let mut cursors = HashMap::new();
        let mut pending = HashMap::new();
        cursors.insert("venear_governance".to_string(), 167_688_827);

        assert_eq!(
            advance_chain_cursor(
                &mut cursors,
                &mut pending,
                "venear_governance",
                167_688_829,
                Some(167_688_827)
            ),
            Some(167_688_829)
        );
        assert_eq!(
            advance_chain_cursor(
                &mut cursors,
                &mut pending,
                "venear_governance",
                167_688_830,
                Some(167_688_829)
            ),
            Some(167_688_830)
        );
        assert_eq!(cursors.get("venear_governance"), Some(&167_688_830));
    }

    #[test]
    fn advance_chain_cursor_infers_prev_height_when_missing() {
        let mut cursors = HashMap::new();
        let mut pending = HashMap::new();
        cursors.insert("raw".to_string(), -1);

        assert_eq!(
            advance_chain_cursor(&mut cursors, &mut pending, "raw", 0, None),
            Some(0)
        );
        assert_eq!(cursors.get("raw"), Some(&0));
    }

    #[test]
    fn upsert_query_has_single_assignment_per_column() {
        let committed_height = 123_i64;
        let stream_name = "near-indexer:venear_governance".to_string();

        let sql = entities::near::near_sync_cursors::Entity::insert(
            entities::near::near_sync_cursors::ActiveModel {
                module_name: Set(stream_name),
                last_processed_height: Set(committed_height),
                updated_at: NotSet,
            },
        )
        .on_conflict(
            OnConflict::column(entities::near::near_sync_cursors::Column::ModuleName)
                .update_column(entities::near::near_sync_cursors::Column::LastProcessedHeight)
                .value(
                    entities::near::near_sync_cursors::Column::UpdatedAt,
                    Expr::current_timestamp(),
                )
                .to_owned(),
        )
        .build(DbBackend::Postgres)
        .to_string();

        assert_eq!(sql.matches("\"last_processed_height\" = ").count(), 1);
        assert_eq!(sql.matches("\"updated_at\" = ").count(), 1);
    }
}
