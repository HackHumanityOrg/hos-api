use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "DROP TABLE IF EXISTS near.venear_governance_relevant_receipts CASCADE",
        )
        .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS near.relevant_receipts (
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
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_relevant_receipts_scope_executed_height
               ON near.relevant_receipts (scope, executed, discovered_at_block_height)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_relevant_receipts_scope_transaction_hash
               ON near.relevant_receipts (scope, transaction_hash)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_relevant_receipts_receipt_id
               ON near.relevant_receipts (receipt_id)",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP TABLE IF EXISTS near.relevant_receipts CASCADE")
            .await?;

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS near.venear_governance_relevant_receipts (
                receipt_id TEXT PRIMARY KEY,
                reason TEXT NOT NULL,
                discovered_by_receipt_id TEXT,
                transaction_hash TEXT,
                discovered_at_block_height BIGINT NOT NULL,
                executed BOOLEAN NOT NULL DEFAULT FALSE,
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_venear_gov_relevant_receipts_executed_height
               ON near.venear_governance_relevant_receipts (executed, discovered_at_block_height)",
        )
        .await?;

        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS ix_venear_gov_relevant_receipts_transaction_hash
               ON near.venear_governance_relevant_receipts (transaction_hash)",
        )
        .await?;

        Ok(())
    }
}
