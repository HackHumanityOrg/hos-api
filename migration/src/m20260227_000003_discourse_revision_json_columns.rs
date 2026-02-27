use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE discourse.discourse_post_revisions ADD COLUMN IF NOT EXISTS body_changes JSONB",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE discourse.discourse_post_revisions ADD COLUMN IF NOT EXISTS title_changes JSONB",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            "ALTER TABLE discourse.discourse_post_revisions DROP COLUMN IF EXISTS title_changes",
        )
        .await?;
        db.execute_unprepared(
            "ALTER TABLE discourse.discourse_post_revisions DROP COLUMN IF EXISTS body_changes",
        )
        .await?;
        Ok(())
    }
}
