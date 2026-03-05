use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "DO $$
            BEGIN
                IF EXISTS (
                    SELECT 1
                    FROM information_schema.tables
                    WHERE table_schema = 'near'
                      AND table_name = 'relevant_receipts'
                )
                AND NOT EXISTS (
                    SELECT 1
                    FROM information_schema.tables
                    WHERE table_schema = 'near'
                      AND table_name = 'near_relevant_receipts'
                ) THEN
                    ALTER TABLE near.relevant_receipts
                    RENAME TO near_relevant_receipts;
                END IF;
            END $$;",
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "DO $$
            BEGIN
                IF EXISTS (
                    SELECT 1
                    FROM information_schema.tables
                    WHERE table_schema = 'near'
                      AND table_name = 'near_relevant_receipts'
                )
                AND NOT EXISTS (
                    SELECT 1
                    FROM information_schema.tables
                    WHERE table_schema = 'near'
                      AND table_name = 'relevant_receipts'
                ) THEN
                    ALTER TABLE near.near_relevant_receipts
                    RENAME TO relevant_receipts;
                END IF;
            END $$;",
        )
        .await?;

        Ok(())
    }
}
