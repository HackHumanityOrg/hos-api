use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // =================
        // NEAR ACCOUNT TRACKING TABLES
        // =================

        // near_accounts - Main account registry
        manager
            .create_table(
                Table::create()
                    .table(NearAccounts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearAccounts::AccountId)
                            .string_len(128)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearAccounts::AccountType)
                            .string_len(20)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccounts::CreatedAtHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccounts::CreatedAtTimestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccounts::DeletedAtHeight)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearAccounts::DeletedAtTimestamp)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearAccounts::ParentAccountId)
                            .string_len(64)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearAccounts::IsContract)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;

        // near_account_balances - Track balance history
        manager
            .create_table(
                Table::create()
                    .table(NearAccountBalances::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearAccountBalances::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearAccountBalances::AccountId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountBalances::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountBalances::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountBalances::Amount)
                            .text()
                            .not_null(), // Store as string to preserve u128 precision
                    )
                    .col(
                        ColumnDef::new(NearAccountBalances::Locked)
                            .text()
                            .not_null(), // Store as string to preserve u128 precision
                    )
                    .col(
                        ColumnDef::new(NearAccountBalances::StorageUsage)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountBalances::CodeHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_access_keys - Track access keys
        manager
            .create_table(
                Table::create()
                    .table(NearAccessKeys::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearAccessKeys::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::AccountId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::PublicKey)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::PermissionType)
                            .string_len(20)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::Allowance).text().null(), // Only for FunctionCall permissions - u128 as string
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::ReceiverId)
                            .string_len(64)
                            .null(), // Only for FunctionCall permissions
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::MethodNames).json().null(), // Array of allowed methods for FunctionCall permissions
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::Nonce)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::CreatedAtHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::DeletedAtHeight)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .to_owned(),
            )
            .await?;

        // near_account_events - Track account lifecycle events
        manager
            .create_table(
                Table::create()
                    .table(NearAccountEvents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearAccountEvents::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearAccountEvents::AccountId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountEvents::EventType)
                            .string_len(50)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountEvents::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountEvents::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountEvents::TransactionHash)
                            .string_len(64)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountEvents::ReceiptId)
                            .string_len(64)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountEvents::Metadata).json().null(), // Additional event data
                    )
                    .to_owned(),
            )
            .await?;

        // =================
        // INDICES FOR PERFORMANCE
        // =================

        // Account balances indices
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_account_balances_account_id")
                    .table(NearAccountBalances::Table)
                    .col(NearAccountBalances::AccountId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_account_balances_block_height")
                    .table(NearAccountBalances::Table)
                    .col(NearAccountBalances::BlockHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_account_balances_timestamp")
                    .table(NearAccountBalances::Table)
                    .col(NearAccountBalances::Timestamp)
                    .to_owned(),
            )
            .await?;

        // Access keys indices
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_access_keys_account_id")
                    .table(NearAccessKeys::Table)
                    .col(NearAccessKeys::AccountId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_access_keys_public_key")
                    .table(NearAccessKeys::Table)
                    .col(NearAccessKeys::PublicKey)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_access_keys_is_active")
                    .table(NearAccessKeys::Table)
                    .col(NearAccessKeys::IsActive)
                    .to_owned(),
            )
            .await?;

        // Account events indices
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_account_events_account_id")
                    .table(NearAccountEvents::Table)
                    .col(NearAccountEvents::AccountId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_account_events_block_height")
                    .table(NearAccountEvents::Table)
                    .col(NearAccountEvents::BlockHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_account_events_event_type")
                    .table(NearAccountEvents::Table)
                    .col(NearAccountEvents::EventType)
                    .to_owned(),
            )
            .await?;

        // Account type and parent indices
        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_accounts_account_type")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::AccountType)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_accounts_parent_account_id")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::ParentAccountId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_accounts_is_contract")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::IsContract)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .if_not_exists()
                    .name("idx_near_accounts_created_at_height")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::CreatedAtHeight)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop tables in reverse order
        manager
            .drop_table(Table::drop().table(NearAccountEvents::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearAccessKeys::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearAccountBalances::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearAccounts::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum NearAccounts {
    Table,
    AccountId,
    AccountType,
    CreatedAtHeight,
    CreatedAtTimestamp,
    DeletedAtHeight,
    DeletedAtTimestamp,
    ParentAccountId,
    IsContract,
}

#[derive(DeriveIden)]
enum NearAccountBalances {
    Table,
    Id,
    AccountId,
    BlockHeight,
    Timestamp,
    Amount,
    Locked,
    StorageUsage,
    CodeHash,
}

#[derive(DeriveIden)]
enum NearAccessKeys {
    Table,
    Id,
    AccountId,
    PublicKey,
    PermissionType,
    Allowance,
    ReceiverId,
    MethodNames,
    Nonce,
    CreatedAtHeight,
    DeletedAtHeight,
    IsActive,
}

#[derive(DeriveIden)]
enum NearAccountEvents {
    Table,
    Id,
    AccountId,
    EventType,
    BlockHeight,
    Timestamp,
    TransactionHash,
    ReceiptId,
    Metadata,
}
