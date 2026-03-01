use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table((Alias::new("telegram"), TelegramImportLogs::Table))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("telegram"), TelegramImportLogs::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TelegramImportLogs::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TelegramImportLogs::FileHash)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(ColumnDef::new(TelegramImportLogs::ChannelId).big_integer())
                    .col(ColumnDef::new(TelegramImportLogs::MessageCount).integer())
                    .col(ColumnDef::new(TelegramImportLogs::UserCount).integer())
                    .col(
                        ColumnDef::new(TelegramImportLogs::Status)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(ColumnDef::new(TelegramImportLogs::ErrorMessage).text())
                    .col(
                        ColumnDef::new(TelegramImportLogs::ImportedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_telegram_import_logs_channel")
                    .from(
                        (Alias::new("telegram"), TelegramImportLogs::Table),
                        TelegramImportLogs::ChannelId,
                    )
                    .to(
                        (Alias::new("telegram"), TelegramChannels::Table),
                        TelegramChannels::Id,
                    )
                    .on_update(ForeignKeyAction::NoAction)
                    .on_delete(ForeignKeyAction::SetNull)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ux_telegram_import_logs_file_hash")
                    .table((Alias::new("telegram"), TelegramImportLogs::Table))
                    .col(TelegramImportLogs::FileHash)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_telegram_import_logs_imported_at")
                    .table((Alias::new("telegram"), TelegramImportLogs::Table))
                    .col(TelegramImportLogs::ImportedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum TelegramImportLogs {
    Table,
    Id,
    FileHash,
    ChannelId,
    MessageCount,
    UserCount,
    Status,
    ErrorMessage,
    ImportedAt,
}

#[derive(DeriveIden)]
enum TelegramChannels {
    Table,
    Id,
}
