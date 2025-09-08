use anyhow::Result;
use sea_orm::prelude::Expr;
use sea_orm::{
    ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    QueryFilter, Set,
};

/// Get the cursor for a specific module
pub async fn get_module_cursor(db: &DatabaseConnection, module_name: &str) -> Result<Option<i64>> {
    let cursor = entities::near_sync_cursors::Entity::find()
        .filter(
            entities::near_sync_cursors::Column::StreamName
                .eq(format!("near-indexer:{}", module_name)),
        )
        .one(db)
        .await?;

    Ok(cursor.map(|c| c.block_height))
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

/// Upsert the cursor for a specific module
pub async fn upsert_module_cursor<C>(db: &C, module_name: &str, height: i64) -> Result<()>
where
    C: ConnectionTrait,
{
    let stream_name = format!("near-indexer:{}", module_name);

    // Try update first and also bump updated_at
    let updated = entities::near_sync_cursors::Entity::update_many()
        .col_expr(
            entities::near_sync_cursors::Column::BlockHeight,
            Expr::value(height),
        )
        .col_expr(
            entities::near_sync_cursors::Column::UpdatedAt,
            Expr::current_timestamp().into(),
        )
        .filter(entities::near_sync_cursors::Column::StreamName.eq(&stream_name))
        .exec(db)
        .await?;

    if updated.rows_affected == 0 {
        // Insert new cursor record
        let c = entities::near_sync_cursors::ActiveModel {
            stream_name: Set(stream_name),
            block_height: Set(height),
            updated_at: NotSet,
        };
        let _ = entities::near_sync_cursors::Entity::insert(c)
            .exec(db)
            .await?;
    }

    Ok(())
}

/// Get the minimum cursor across all enabled modules
pub async fn get_min_cursor_across_modules(
    db: &DatabaseConnection,
    module_names: &[&str],
) -> Result<Option<i64>> {
    if module_names.is_empty() {
        return Ok(None);
    }

    let stream_names: Vec<String> = module_names
        .iter()
        .map(|name| format!("near-indexer:{}", name))
        .collect();

    let cursors = entities::near_sync_cursors::Entity::find()
        .filter(entities::near_sync_cursors::Column::StreamName.is_in(stream_names))
        .all(db)
        .await?;

    // If we don't have cursors for all modules, return None (start from beginning)
    if cursors.len() != module_names.len() {
        return Ok(None);
    }

    // Find the minimum height
    let min_height = cursors
        .into_iter()
        .map(|c| c.block_height)
        .min()
        .unwrap_or(0);

    Ok(Some(min_height))
}

/// Check if a module can process a block given the current state of all modules
pub async fn can_module_process_block(
    db: &DatabaseConnection,
    module_name: &str,
    block_height: i64,
    all_module_names: &[&str],
) -> Result<bool> {
    use tracing::debug;

    // Get current cursor for this module
    let current_cursor = get_module_cursor(db, module_name).await?;

    debug!(
        module = module_name,
        block_height, current_cursor, "Checking if module can process block"
    );

    // Check if this module has already processed this block
    if let Some(cursor) = current_cursor {
        if cursor >= block_height {
            debug!(
                module = module_name,
                block_height, cursor, "Module already processed this block"
            );
            return Ok(false); // Already processed
        }
    }

    // Get minimum cursor across all modules
    let min_cursor = get_min_cursor_across_modules(db, all_module_names).await?;

    debug!(
        module = module_name,
        block_height,
        min_cursor,
        all_modules = ?all_module_names,
        "Min cursor across all modules"
    );

    // If no cursors exist, all modules can start
    let Some(min_cursor) = min_cursor else {
        debug!(
            module = module_name,
            block_height, "No minimum cursor found - allowing module to process"
        );
        return Ok(true);
    };

    // Module can process if:
    // 1. It needs to process this block (cursor < block_height)
    // 2. Its current position is not too far ahead of the minimum cursor
    let current_module_cursor = current_cursor.unwrap_or(-1);

    // Allow processing if this module needs to process this block
    // AND the module isn't too far ahead of the slowest module
    let needs_this_block = current_module_cursor < block_height;
    let not_too_far_ahead = current_module_cursor <= min_cursor;

    let can_process = needs_this_block && not_too_far_ahead;

    debug!(
        module = module_name,
        block_height,
        current_module_cursor,
        min_cursor,
        needs_this_block,
        not_too_far_ahead,
        can_process,
        "Module processing decision"
    );

    Ok(can_process)
}
