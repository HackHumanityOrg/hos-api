use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("CREATE SCHEMA IF NOT EXISTS telegram")
            .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("telegram"), TelegramChannels::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TelegramChannels::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TelegramChannels::TelegramId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(TelegramChannels::Name).text().not_null())
                    .col(ColumnDef::new(TelegramChannels::Username).text())
                    .col(
                        ColumnDef::new(TelegramChannels::Type)
                            .string_len(50)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TelegramChannels::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(TelegramChannels::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ux_telegram_channels_telegram_id")
                    .table((Alias::new("telegram"), TelegramChannels::Table))
                    .col(TelegramChannels::TelegramId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("telegram"), TelegramUsers::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TelegramUsers::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TelegramUsers::TelegramId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(TelegramUsers::Username).text())
                    .col(ColumnDef::new(TelegramUsers::DisplayName).text())
                    .col(
                        ColumnDef::new(TelegramUsers::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(TelegramUsers::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ux_telegram_users_telegram_id")
                    .table((Alias::new("telegram"), TelegramUsers::Table))
                    .col(TelegramUsers::TelegramId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("telegram"), TelegramMessages::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TelegramMessages::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TelegramMessages::ChannelId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TelegramMessages::TelegramMessageId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TelegramMessages::MessageType)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(ColumnDef::new(TelegramMessages::FromUserId).big_integer())
                    .col(ColumnDef::new(TelegramMessages::ReplyToMessageId).big_integer())
                    .col(ColumnDef::new(TelegramMessages::ReplyToTelegramMessageId).big_integer())
                    .col(
                        ColumnDef::new(TelegramMessages::Date)
                            .timestamp()
                            .not_null(),
                    )
                    .col(ColumnDef::new(TelegramMessages::EditedDate).timestamp())
                    .col(ColumnDef::new(TelegramMessages::Text).text())
                    .col(ColumnDef::new(TelegramMessages::TextEntities).json_binary())
                    .col(
                        ColumnDef::new(TelegramMessages::HasMedia)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(TelegramMessages::MediaType).string_len(64))
                    .col(ColumnDef::new(TelegramMessages::MediaPath).text())
                    .col(
                        ColumnDef::new(TelegramMessages::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(TelegramMessages::UpdatedAt)
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
                    .name("fk_telegram_messages_channel")
                    .from(
                        (Alias::new("telegram"), TelegramMessages::Table),
                        TelegramMessages::ChannelId,
                    )
                    .to(
                        (Alias::new("telegram"), TelegramChannels::Table),
                        TelegramChannels::Id,
                    )
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_telegram_messages_user")
                    .from(
                        (Alias::new("telegram"), TelegramMessages::Table),
                        TelegramMessages::FromUserId,
                    )
                    .to(
                        (Alias::new("telegram"), TelegramUsers::Table),
                        TelegramUsers::Id,
                    )
                    .on_update(ForeignKeyAction::NoAction)
                    .on_delete(ForeignKeyAction::SetNull)
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_telegram_messages_reply")
                    .from(
                        (Alias::new("telegram"), TelegramMessages::Table),
                        TelegramMessages::ReplyToMessageId,
                    )
                    .to(
                        (Alias::new("telegram"), TelegramMessages::Table),
                        TelegramMessages::Id,
                    )
                    .on_update(ForeignKeyAction::NoAction)
                    .on_delete(ForeignKeyAction::SetNull)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ux_telegram_messages_channel_message")
                    .table((Alias::new("telegram"), TelegramMessages::Table))
                    .col(TelegramMessages::ChannelId)
                    .col(TelegramMessages::TelegramMessageId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_telegram_messages_channel_date")
                    .table((Alias::new("telegram"), TelegramMessages::Table))
                    .col(TelegramMessages::ChannelId)
                    .col(TelegramMessages::Date)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_telegram_messages_from_user")
                    .table((Alias::new("telegram"), TelegramMessages::Table))
                    .col(TelegramMessages::FromUserId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_telegram_messages_reply_lookup")
                    .table((Alias::new("telegram"), TelegramMessages::Table))
                    .col(TelegramMessages::ChannelId)
                    .col(TelegramMessages::ReplyToTelegramMessageId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("telegram"), TelegramReactions::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TelegramReactions::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TelegramReactions::MessageId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TelegramReactions::UserId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TelegramReactions::Emoji)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TelegramReactions::Date)
                            .timestamp()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TelegramReactions::CreatedAt)
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
                    .name("fk_telegram_reactions_message")
                    .from(
                        (Alias::new("telegram"), TelegramReactions::Table),
                        TelegramReactions::MessageId,
                    )
                    .to(
                        (Alias::new("telegram"), TelegramMessages::Table),
                        TelegramMessages::Id,
                    )
                    .on_update(ForeignKeyAction::NoAction)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_telegram_reactions_user")
                    .from(
                        (Alias::new("telegram"), TelegramReactions::Table),
                        TelegramReactions::UserId,
                    )
                    .to(
                        (Alias::new("telegram"), TelegramUsers::Table),
                        TelegramUsers::Id,
                    )
                    .on_update(ForeignKeyAction::NoAction)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ux_telegram_reactions_message_user_emoji")
                    .table((Alias::new("telegram"), TelegramReactions::Table))
                    .col(TelegramReactions::MessageId)
                    .col(TelegramReactions::UserId)
                    .col(TelegramReactions::Emoji)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_telegram_reactions_message")
                    .table((Alias::new("telegram"), TelegramReactions::Table))
                    .col(TelegramReactions::MessageId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("telegram"), TelegramServiceActions::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TelegramServiceActions::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(TelegramServiceActions::MessageId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(TelegramServiceActions::Action)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(ColumnDef::new(TelegramServiceActions::ActorId).big_integer())
                    .col(ColumnDef::new(TelegramServiceActions::Metadata).json_binary())
                    .col(
                        ColumnDef::new(TelegramServiceActions::CreatedAt)
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
                    .name("fk_telegram_service_actions_message")
                    .from(
                        (Alias::new("telegram"), TelegramServiceActions::Table),
                        TelegramServiceActions::MessageId,
                    )
                    .to(
                        (Alias::new("telegram"), TelegramMessages::Table),
                        TelegramMessages::Id,
                    )
                    .on_update(ForeignKeyAction::NoAction)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_telegram_service_actions_actor")
                    .from(
                        (Alias::new("telegram"), TelegramServiceActions::Table),
                        TelegramServiceActions::ActorId,
                    )
                    .to(
                        (Alias::new("telegram"), TelegramUsers::Table),
                        TelegramUsers::Id,
                    )
                    .on_update(ForeignKeyAction::NoAction)
                    .on_delete(ForeignKeyAction::SetNull)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ux_telegram_service_actions_message_action")
                    .table((Alias::new("telegram"), TelegramServiceActions::Table))
                    .col(TelegramServiceActions::MessageId)
                    .col(TelegramServiceActions::Action)
                    .unique()
                    .to_owned(),
            )
            .await?;

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

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table((Alias::new("telegram"), TelegramImportLogs::Table))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table((Alias::new("telegram"), TelegramServiceActions::Table))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table((Alias::new("telegram"), TelegramReactions::Table))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table((Alias::new("telegram"), TelegramMessages::Table))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table((Alias::new("telegram"), TelegramUsers::Table))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table((Alias::new("telegram"), TelegramChannels::Table))
                    .to_owned(),
            )
            .await?;

        let db = manager.get_connection();
        db.execute_unprepared("DROP SCHEMA IF EXISTS telegram CASCADE")
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum TelegramChannels {
    Table,
    Id,
    TelegramId,
    Name,
    Username,
    Type,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TelegramUsers {
    Table,
    Id,
    TelegramId,
    Username,
    DisplayName,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TelegramMessages {
    Table,
    Id,
    ChannelId,
    TelegramMessageId,
    MessageType,
    FromUserId,
    ReplyToMessageId,
    ReplyToTelegramMessageId,
    Date,
    EditedDate,
    Text,
    TextEntities,
    HasMedia,
    MediaType,
    MediaPath,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TelegramReactions {
    Table,
    Id,
    MessageId,
    UserId,
    Emoji,
    Date,
    CreatedAt,
}

#[derive(DeriveIden)]
enum TelegramServiceActions {
    Table,
    Id,
    MessageId,
    Action,
    ActorId,
    Metadata,
    CreatedAt,
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
