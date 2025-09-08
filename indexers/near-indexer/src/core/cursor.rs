use anyhow::Result;
use sea_orm::prelude::Expr;
use sea_orm::{
    ActiveValue::NotSet, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait,
    QueryFilter, Set,
};
use sea_orm::sea_query::OnConflict;

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

/// Upsert the cursor for a specific module (parallel-safe)
pub async fn upsert_module_cursor<C>(db: &C, module_name: &str, height: i64) -> Result<()>
where
    C: ConnectionTrait,
{
    let stream_name = format!("near-indexer:{}", module_name);

    // Use ON DUPLICATE KEY UPDATE to handle parallel cursor updates safely
    // This will insert if not exists, or update if exists
    let cursor_record = entities::near_sync_cursors::ActiveModel {
        stream_name: Set(stream_name),
        block_height: Set(height),
        updated_at: NotSet, // Will be set by database default/trigger
    };

    // Use on_conflict to handle parallel inserts gracefully
    // For MySQL this generates: INSERT ... ON DUPLICATE KEY UPDATE block_height = VALUES(block_height)
    entities::near_sync_cursors::Entity::insert(cursor_record)
        .on_conflict(
            OnConflict::column(entities::near_sync_cursors::Column::StreamName)
                .update_columns([
                    entities::near_sync_cursors::Column::BlockHeight,
                    entities::near_sync_cursors::Column::UpdatedAt,
                ])
                .values([
                    (entities::near_sync_cursors::Column::BlockHeight, Expr::value(height).into()),
                    (entities::near_sync_cursors::Column::UpdatedAt, Expr::current_timestamp().into()),
                ])
                .to_owned(),
        )
        .exec(db)
        .await?;

    Ok(())
}

