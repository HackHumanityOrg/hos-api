use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // This is a placeholder migration - no tables to create yet
        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // This is a placeholder migration - nothing to rollback
        Ok(())
    }
}

// Placeholder enum - remove when creating real tables
#[derive(DeriveIden)]
enum _Post {
    _Table,
}
