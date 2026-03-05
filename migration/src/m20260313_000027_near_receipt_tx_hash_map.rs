use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let statements: &[&str] = &[
            "CREATE TABLE IF NOT EXISTS near.near_receipt_tx_hash_map (
                receipt_id TEXT PRIMARY KEY,
                transaction_hash TEXT NOT NULL,
                first_seen_height BIGINT NOT NULL,
                last_seen_height BIGINT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
            "CREATE INDEX IF NOT EXISTS ix_near_receipt_tx_hash_map_tx_hash
               ON near.near_receipt_tx_hash_map (transaction_hash)",
            "CREATE INDEX IF NOT EXISTS ix_near_receipt_tx_hash_map_last_seen_height
               ON near.near_receipt_tx_hash_map (last_seen_height)",
            "DO $$
             BEGIN
                 IF EXISTS (
                     SELECT 1
                     FROM information_schema.tables
                     WHERE table_schema = 'near'
                       AND table_name = 'near_relevant_receipts'
                 ) THEN
                     INSERT INTO near.near_receipt_tx_hash_map (
                         receipt_id,
                         transaction_hash,
                         first_seen_height,
                         last_seen_height,
                         created_at,
                         updated_at
                     )
                     SELECT
                         rr.receipt_id,
                         rr.transaction_hash,
                         rr.discovered_at_block_height,
                         rr.discovered_at_block_height,
                         NOW(),
                         NOW()
                     FROM near.near_relevant_receipts rr
                     WHERE rr.transaction_hash IS NOT NULL
                     ON CONFLICT (receipt_id) DO UPDATE
                     SET last_seen_height = GREATEST(
                             near.near_receipt_tx_hash_map.last_seen_height,
                             EXCLUDED.last_seen_height
                         ),
                         updated_at = NOW();
                 END IF;
             END $$",
            "DROP TABLE IF EXISTS near.near_relevant_receipts CASCADE",
        ];

        for sql in statements {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let statements: &[&str] = &[
            "CREATE TABLE IF NOT EXISTS near.near_relevant_receipts (
                scope TEXT NOT NULL,
                receipt_id TEXT NOT NULL,
                reason TEXT NOT NULL,
                discovered_by_receipt_id TEXT,
                transaction_hash TEXT,
                discovered_at_block_height BIGINT NOT NULL,
                executed BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (scope, receipt_id)
            )",
            "CREATE INDEX IF NOT EXISTS ix_relevant_receipts_scope_executed_height
               ON near.near_relevant_receipts (scope, executed, discovered_at_block_height)",
            "CREATE INDEX IF NOT EXISTS ix_relevant_receipts_scope_transaction_hash
               ON near.near_relevant_receipts (scope, transaction_hash)",
            "CREATE INDEX IF NOT EXISTS ix_relevant_receipts_receipt_id
               ON near.near_relevant_receipts (receipt_id)",
            "INSERT INTO near.near_relevant_receipts (
                scope,
                receipt_id,
                reason,
                discovered_by_receipt_id,
                transaction_hash,
                discovered_at_block_height,
                executed,
                created_at,
                updated_at
            )
            SELECT
                'all',
                map.receipt_id,
                'observed',
                NULL,
                map.transaction_hash,
                map.first_seen_height,
                FALSE,
                NOW(),
                NOW()
            FROM near.near_receipt_tx_hash_map map
            ON CONFLICT (scope, receipt_id) DO UPDATE
            SET transaction_hash = COALESCE(
                    near.near_relevant_receipts.transaction_hash,
                    EXCLUDED.transaction_hash
                ),
                discovered_at_block_height = LEAST(
                    near.near_relevant_receipts.discovered_at_block_height,
                    EXCLUDED.discovered_at_block_height
                ),
                updated_at = NOW()",
            "DROP INDEX IF EXISTS near.ix_near_receipt_tx_hash_map_last_seen_height",
            "DROP INDEX IF EXISTS near.ix_near_receipt_tx_hash_map_tx_hash",
            "DROP TABLE IF EXISTS near.near_receipt_tx_hash_map",
        ];

        for sql in statements {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }
}
