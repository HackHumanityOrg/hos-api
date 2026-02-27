use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table((Alias::new("discourse"), DiscoursePostRevisions::Table))
                    .add_column(
                        ColumnDef::new(DiscoursePostRevisions::BodyChanges)
                            .json_binary()
                            .null(),
                    )
                    .add_column(
                        ColumnDef::new(DiscoursePostRevisions::TitleChanges)
                            .json_binary()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table((Alias::new("discourse"), DiscoursePostRevisions::Table))
                    .drop_column(DiscoursePostRevisions::TitleChanges)
                    .drop_column(DiscoursePostRevisions::BodyChanges)
                    .to_owned(),
            )
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum DiscoursePostRevisions {
    Table,
    BodyChanges,
    TitleChanges,
}
