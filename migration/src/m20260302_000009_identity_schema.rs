use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("CREATE SCHEMA IF NOT EXISTS identity")
            .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("identity"), Alias::new("platforms")))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(IdentityPlatforms::Id)
                            .string_len(32)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(IdentityPlatforms::Label)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdentityPlatforms::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        db.execute_unprepared(
            r#"
            INSERT INTO identity.platforms (id, label)
            VALUES
                ('discourse', 'Discourse'),
                ('near', 'NEAR'),
                ('telegram', 'Telegram')
            ON CONFLICT (id) DO NOTHING
            "#,
        )
        .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("identity"), Alias::new("persons")))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(IdentityPersons::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(IdentityPersons::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(IdentityPersons::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("identity"), Alias::new("accounts")))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(IdentityAccounts::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(IdentityAccounts::PlatformId)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdentityAccounts::Scope)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdentityAccounts::ExternalId)
                            .string_len(255)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdentityAccounts::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(IdentityAccounts::UpdatedAt)
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
                    .name("fk_identity_accounts_platform")
                    .from(
                        (Alias::new("identity"), Alias::new("accounts")),
                        IdentityAccounts::PlatformId,
                    )
                    .to(
                        (Alias::new("identity"), Alias::new("platforms")),
                        IdentityPlatforms::Id,
                    )
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Restrict)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ux_identity_accounts_platform_scope_external")
                    .table((Alias::new("identity"), Alias::new("accounts")))
                    .col(IdentityAccounts::PlatformId)
                    .col(IdentityAccounts::Scope)
                    .col(IdentityAccounts::ExternalId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_identity_accounts_platform_scope")
                    .table((Alias::new("identity"), Alias::new("accounts")))
                    .col(IdentityAccounts::PlatformId)
                    .col(IdentityAccounts::Scope)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("identity"), Alias::new("person_accounts")))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(IdentityPersonAccounts::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(IdentityPersonAccounts::PersonId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdentityPersonAccounts::AccountId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdentityPersonAccounts::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(IdentityPersonAccounts::UpdatedAt)
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
                    .name("fk_identity_person_accounts_person")
                    .from(
                        (Alias::new("identity"), Alias::new("person_accounts")),
                        IdentityPersonAccounts::PersonId,
                    )
                    .to(
                        (Alias::new("identity"), Alias::new("persons")),
                        IdentityPersons::Id,
                    )
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_identity_person_accounts_account")
                    .from(
                        (Alias::new("identity"), Alias::new("person_accounts")),
                        IdentityPersonAccounts::AccountId,
                    )
                    .to(
                        (Alias::new("identity"), Alias::new("accounts")),
                        IdentityAccounts::Id,
                    )
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ux_identity_person_accounts_account")
                    .table((Alias::new("identity"), Alias::new("person_accounts")))
                    .col(IdentityPersonAccounts::AccountId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_identity_person_accounts_person")
                    .table((Alias::new("identity"), Alias::new("person_accounts")))
                    .col(IdentityPersonAccounts::PersonId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("identity"), Alias::new("account_aliases")))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(IdentityAccountAliases::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(IdentityAccountAliases::AccountId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdentityAccountAliases::Alias)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdentityAccountAliases::AliasNormalized)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(IdentityAccountAliases::FirstSeenAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(IdentityAccountAliases::LastSeenAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(IdentityAccountAliases::IsCurrent)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(
                        ColumnDef::new(IdentityAccountAliases::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(IdentityAccountAliases::UpdatedAt)
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
                    .name("fk_identity_account_aliases_account")
                    .from(
                        (Alias::new("identity"), Alias::new("account_aliases")),
                        IdentityAccountAliases::AccountId,
                    )
                    .to(
                        (Alias::new("identity"), Alias::new("accounts")),
                        IdentityAccounts::Id,
                    )
                    .on_update(ForeignKeyAction::Cascade)
                    .on_delete(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ux_identity_account_aliases_account_alias")
                    .table((Alias::new("identity"), Alias::new("account_aliases")))
                    .col(IdentityAccountAliases::AccountId)
                    .col(IdentityAccountAliases::AliasNormalized)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_identity_account_aliases_alias")
                    .table((Alias::new("identity"), Alias::new("account_aliases")))
                    .col(IdentityAccountAliases::AliasNormalized)
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
                    .table((Alias::new("identity"), Alias::new("account_aliases")))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table((Alias::new("identity"), Alias::new("person_accounts")))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table((Alias::new("identity"), Alias::new("accounts")))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table((Alias::new("identity"), Alias::new("persons")))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table((Alias::new("identity"), Alias::new("platforms")))
                    .to_owned(),
            )
            .await?;

        let db = manager.get_connection();
        db.execute_unprepared("DROP SCHEMA IF EXISTS identity CASCADE")
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum IdentityPlatforms {
    Id,
    Label,
    CreatedAt,
}

#[derive(DeriveIden)]
enum IdentityPersons {
    Id,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum IdentityAccounts {
    Id,
    PlatformId,
    Scope,
    ExternalId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum IdentityPersonAccounts {
    Id,
    PersonId,
    AccountId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum IdentityAccountAliases {
    Id,
    AccountId,
    Alias,
    AliasNormalized,
    FirstSeenAt,
    LastSeenAt,
    IsCurrent,
    CreatedAt,
    UpdatedAt,
}
