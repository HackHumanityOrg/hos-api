use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let statements: &[&str] = &[
            // Fix state_changes duplication: deduplicate existing rows then add
            // unique constraint so reprocessing a block cannot create duplicates.
            "DELETE FROM near.near_state_changes AS a
             USING near.near_state_changes AS b
             WHERE a.id > b.id
               AND a.block_height = b.block_height
               AND a.shard_id = b.shard_id",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_near_state_changes_block_shard
             ON near.near_state_changes (block_height, shard_id)",
            // Add raw_data JSONB to blocks and chunks so all upstream fields are
            // preserved (enables local replay without S3).
            "ALTER TABLE near.near_blocks ADD COLUMN IF NOT EXISTS raw_data JSONB",
            "ALTER TABLE near.near_chunks ADD COLUMN IF NOT EXISTS raw_data JSONB",
            // Add shard_id to transactions, receipts, and execution_outcomes so
            // we can reconstruct the per-shard StreamerMessage structure.
            "ALTER TABLE near.near_transactions ADD COLUMN IF NOT EXISTS shard_id BIGINT",
            "ALTER TABLE near.near_receipts ADD COLUMN IF NOT EXISTS shard_id BIGINT",
            "ALTER TABLE near.near_execution_outcomes ADD COLUMN IF NOT EXISTS shard_id BIGINT",
            "ALTER TABLE near.near_transactions ALTER COLUMN shard_id SET NOT NULL",
            "ALTER TABLE near.near_receipts ALTER COLUMN shard_id SET NOT NULL",
            "ALTER TABLE near.near_execution_outcomes ALTER COLUMN shard_id SET NOT NULL",
            // Secondary indexes for query performance on raw tables.
            "CREATE INDEX IF NOT EXISTS ix_near_chunks_block_height_shard_id
             ON near.near_chunks (block_height, shard_id)",
            "CREATE INDEX IF NOT EXISTS ix_near_transactions_block_height_shard_id
             ON near.near_transactions (block_height, shard_id)",
            "CREATE INDEX IF NOT EXISTS ix_near_transactions_signer_id
             ON near.near_transactions (signer_id)",
            "CREATE INDEX IF NOT EXISTS ix_near_transactions_receiver_id
             ON near.near_transactions (receiver_id)",
            "CREATE INDEX IF NOT EXISTS ix_near_receipts_block_height_shard_id
             ON near.near_receipts (block_height, shard_id)",
            "CREATE INDEX IF NOT EXISTS ix_near_receipts_predecessor_id
             ON near.near_receipts (predecessor_id)",
            "CREATE INDEX IF NOT EXISTS ix_near_receipts_receiver_id
             ON near.near_receipts (receiver_id)",
            "CREATE INDEX IF NOT EXISTS ix_near_execution_outcomes_block_height_shard_id
             ON near.near_execution_outcomes (block_height, shard_id)",
            "CREATE INDEX IF NOT EXISTS ix_near_execution_outcomes_receipt_id
             ON near.near_execution_outcomes (receipt_id)",
        ];

        for sql in statements {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let statements: &[&str] = &[
            "DROP INDEX IF EXISTS near.ix_near_execution_outcomes_receipt_id",
            "DROP INDEX IF EXISTS near.ix_near_execution_outcomes_block_height_shard_id",
            "DROP INDEX IF EXISTS near.ix_near_receipts_receiver_id",
            "DROP INDEX IF EXISTS near.ix_near_receipts_predecessor_id",
            "DROP INDEX IF EXISTS near.ix_near_receipts_block_height_shard_id",
            "DROP INDEX IF EXISTS near.ix_near_transactions_receiver_id",
            "DROP INDEX IF EXISTS near.ix_near_transactions_signer_id",
            "DROP INDEX IF EXISTS near.ix_near_transactions_block_height_shard_id",
            "DROP INDEX IF EXISTS near.ix_near_chunks_block_height_shard_id",
            "ALTER TABLE near.near_execution_outcomes DROP COLUMN IF EXISTS shard_id",
            "ALTER TABLE near.near_receipts DROP COLUMN IF EXISTS shard_id",
            "ALTER TABLE near.near_transactions DROP COLUMN IF EXISTS shard_id",
            "ALTER TABLE near.near_chunks DROP COLUMN IF EXISTS raw_data",
            "ALTER TABLE near.near_blocks DROP COLUMN IF EXISTS raw_data",
            "DROP INDEX IF EXISTS near.ux_near_state_changes_block_shard",
        ];

        for sql in statements {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }
}
