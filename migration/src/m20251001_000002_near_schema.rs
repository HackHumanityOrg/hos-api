use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create near schema
        let db = manager.get_connection();
        db.execute_unprepared("CREATE SCHEMA IF NOT EXISTS near")
            .await?;

        // =====================
        // RAW BLOCKCHAIN DATA
        // =====================

        // near_blocks
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearBlocks::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearBlocks::Height)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(NearBlocks::Hash).string_len(64).not_null())
                    .col(
                        ColumnDef::new(NearBlocks::PrevHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearBlocks::EpochId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearBlocks::NextEpochId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearBlocks::Author).text().not_null())
                    .col(
                        ColumnDef::new(NearBlocks::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearBlocks::TimestampNanosec)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearBlocks::GasPrice).text().not_null())
                    .col(ColumnDef::new(NearBlocks::TotalSupply).text().not_null())
                    .col(
                        ColumnDef::new(NearBlocks::ValidatorReward)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearBlocks::ProtocolVersion)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearBlocks::ChunksIncluded)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearBlocks::BlockOrdinal)
                            .big_integer()
                            .null(),
                    )
                    .col(ColumnDef::new(NearBlocks::PrevHeight).big_integer().null())
                    .to_owned(),
            )
            .await?;

        // near_chunks
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearChunks::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearChunks::ChunkHash)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearChunks::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearChunks::ShardId).big_integer().not_null())
                    .col(ColumnDef::new(NearChunks::Author).text().not_null())
                    .col(
                        ColumnDef::new(NearChunks::HeightCreated)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearChunks::HeightIncluded)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearChunks::GasUsed).big_integer().not_null())
                    .col(
                        ColumnDef::new(NearChunks::GasLimit)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearChunks::ValidatorReward)
                            .text()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_transactions
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearTransactions::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearTransactions::Hash)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearTransactions::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearTransactions::SignerId).text().not_null())
                    .col(
                        ColumnDef::new(NearTransactions::ReceiverId)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTransactions::Nonce)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTransactions::PriorityFee)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTransactions::PublicKey)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTransactions::Signature)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearTransactions::Actions).json().not_null())
                    .col(ColumnDef::new(NearTransactions::RawData).json().null())
                    .to_owned(),
            )
            .await?;

        // near_receipts
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearReceipts::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearReceipts::ReceiptId)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearReceipts::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearReceipts::PredecessorId)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearReceipts::ReceiverId).text().not_null())
                    .col(ColumnDef::new(NearReceipts::Receipt).json().not_null())
                    .col(ColumnDef::new(NearReceipts::RawData).json().null())
                    .to_owned(),
            )
            .await?;

        // near_execution_outcomes
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearExecutionOutcomes::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearExecutionOutcomes::Id)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearExecutionOutcomes::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearExecutionOutcomes::ReceiptId)
                            .string_len(64)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearExecutionOutcomes::Outcome)
                            .json()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearExecutionOutcomes::RawData).json().null())
                    .to_owned(),
            )
            .await?;

        // near_state_changes
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearStateChanges::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearStateChanges::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearStateChanges::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStateChanges::ShardId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStateChanges::StateChanges)
                            .json()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_sync_cursors
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearSyncCursors::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearSyncCursors::ModuleName)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearSyncCursors::LastProcessedHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearSyncCursors::UpdatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // =====================
        // ACCOUNT TABLES
        // =====================

        // near_accounts
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearAccounts::Table))
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

        // near_account_balances
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearAccountBalances::Table))
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
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountBalances::Locked)
                            .text()
                            .not_null(),
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

        // near_access_keys
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearAccessKeys::Table))
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
                    .col(ColumnDef::new(NearAccessKeys::Allowance).text().null())
                    .col(
                        ColumnDef::new(NearAccessKeys::ReceiverId)
                            .string_len(64)
                            .null(),
                    )
                    .col(ColumnDef::new(NearAccessKeys::MethodNames).json().null())
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

        // near_account_events
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearAccountEvents::Table))
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
                    .col(ColumnDef::new(NearAccountEvents::Metadata).json().null())
                    .to_owned(),
            )
            .await?;

        // =====================
        // TOKEN TABLES
        // =====================

        // near_token_events_raw
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearTokenEventsRaw::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearTokenEventsRaw::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEventsRaw::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEventsRaw::BlockTimestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEventsRaw::ShardId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEventsRaw::ReceiptId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEventsRaw::ExecutionOutcomeId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEventsRaw::PredecessorAccountId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEventsRaw::ContractId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEventsRaw::LogIndex)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEventsRaw::LogText)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEventsRaw::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // near_token_events
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearTokenEvents::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearTokenEvents::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::RawEventId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::BlockTimestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::ContractId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::Standard)
                            .string_len(10)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::Version)
                            .string_len(10)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::EventType)
                            .string_len(20)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearTokenEvents::EventData).json().not_null())
                    .col(
                        ColumnDef::new(NearTokenEvents::AffectedAccounts)
                            .text()
                            .not_null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_near_token_events_raw_event_id")
                            .from(
                                (Alias::new("near"), NearTokenEvents::Table),
                                NearTokenEvents::RawEventId,
                            )
                            .to(
                                (Alias::new("near"), NearTokenEventsRaw::Table),
                                NearTokenEventsRaw::Id,
                            )
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // near_token_contracts
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearTokenContracts::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearTokenContracts::ContractId)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearTokenContracts::StandardsDetected)
                            .json()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearTokenContracts::FtMetadata).json())
                    .col(ColumnDef::new(NearTokenContracts::NftMetadata).json())
                    .col(ColumnDef::new(NearTokenContracts::MtMetadata).json())
                    .col(
                        ColumnDef::new(NearTokenContracts::FirstSeenBlock)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenContracts::LastActivityBlock)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenContracts::TotalEvents)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_token_balances
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearTokenBalances::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearTokenBalances::ContractId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearTokenBalances::TokenTypeId).string_len(255))
                    .col(ColumnDef::new(NearTokenBalances::TokenId).string_len(255))
                    .col(
                        ColumnDef::new(NearTokenBalances::AccountId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearTokenBalances::Amount).text().not_null())
                    .col(
                        ColumnDef::new(NearTokenBalances::Standard)
                            .string_len(10)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenBalances::LastUpdatedBlock)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearTokenBalances::Metadata).json())
                    .primary_key(
                        Index::create()
                            .col(NearTokenBalances::ContractId)
                            .col(NearTokenBalances::TokenTypeId)
                            .col(NearTokenBalances::TokenId)
                            .col(NearTokenBalances::AccountId),
                    )
                    .to_owned(),
            )
            .await?;

        // near_token_transfers
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearTokenTransfers::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearTokenTransfers::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearTokenTransfers::EventId)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenTransfers::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenTransfers::BlockTimestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenTransfers::ContractId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenTransfers::Standard)
                            .string_len(10)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearTokenTransfers::TokenTypeId).string_len(255))
                    .col(ColumnDef::new(NearTokenTransfers::TokenId).string_len(255))
                    .col(ColumnDef::new(NearTokenTransfers::FromAccount).string_len(64))
                    .col(ColumnDef::new(NearTokenTransfers::ToAccount).string_len(64))
                    .col(ColumnDef::new(NearTokenTransfers::Amount).text().not_null())
                    .col(ColumnDef::new(NearTokenTransfers::Memo).text())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_near_token_transfers_event_id")
                            .from(
                                (Alias::new("near"), NearTokenTransfers::Table),
                                NearTokenTransfers::EventId,
                            )
                            .to(
                                (Alias::new("near"), NearTokenEvents::Table),
                                NearTokenEvents::Id,
                            )
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // near_token_metadata
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearTokenMetadata::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearTokenMetadata::ContractId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearTokenMetadata::TokenTypeId).string_len(255))
                    .col(ColumnDef::new(NearTokenMetadata::TokenId).string_len(255))
                    .col(
                        ColumnDef::new(NearTokenMetadata::Standard)
                            .string_len(10)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenMetadata::Metadata)
                            .json()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearTokenMetadata::LastUpdatedBlock).big_integer())
                    .primary_key(
                        Index::create()
                            .col(NearTokenMetadata::ContractId)
                            .col(NearTokenMetadata::TokenTypeId)
                            .col(NearTokenMetadata::TokenId),
                    )
                    .to_owned(),
            )
            .await?;

        // =====================
        // STAKING TABLES
        // =====================

        // near_staking_pools
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearStakingPools::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearStakingPools::PoolId)
                            .string_len(128)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPools::PoolType)
                            .string_len(20)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPools::DetectedMethod)
                            .string_len(50)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPools::FirstSeenHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPools::LastActivityHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPools::IsActive)
                            .boolean()
                            .not_null()
                            .default(true),
                    )
                    .col(ColumnDef::new(NearStakingPools::Metadata).json().null())
                    .to_owned(),
            )
            .await?;

        // near_staking_pool_actions
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearStakingPoolActions::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearStakingPoolActions::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolActions::PoolId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolActions::MethodName)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolActions::CallerAccountId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolActions::ArgsJson)
                            .json()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolActions::ArgsBase64)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolActions::AttachedDeposit)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolActions::TransactionHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolActions::ReceiptId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolActions::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolActions::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolActions::Success)
                            .boolean()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolActions::ActionType)
                            .string_len(20)
                            .null(),
                    )
                    .col(ColumnDef::new(NearStakingPoolActions::Amount).text().null())
                    .to_owned(),
            )
            .await?;

        // near_staking_pool_events
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearStakingPoolEvents::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearStakingPoolEvents::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolEvents::PoolId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolEvents::EventStandard)
                            .string_len(20)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolEvents::EventType)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolEvents::EventData)
                            .json()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolEvents::RawLog)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolEvents::LogIndex)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolEvents::ExecutionOutcomeId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolEvents::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolEvents::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_staking_delegators
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearStakingDelegators::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearStakingDelegators::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearStakingDelegators::PoolId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingDelegators::DelegatorAccountId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingDelegators::FirstInteractionHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingDelegators::FirstInteractionType)
                            .string_len(20)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingDelegators::LastInteractionHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingDelegators::LastInteractionType)
                            .string_len(20)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingDelegators::InteractionCount)
                            .integer()
                            .not_null()
                            .default(1),
                    )
                    .to_owned(),
            )
            .await?;

        // near_staking_epochs
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearStakingEpochs::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearStakingEpochs::EpochId)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::EpochHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::StartBlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::EndBlockHeight)
                            .big_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::ProtocolVersion)
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::TotalStake)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::TotalValidatorReward)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::ValidatorsInfo)
                            .json()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::ChunkProducersSettlement)
                            .json()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::EpochDurationBlocks)
                            .big_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_staking_validator_proposals
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearStakingValidatorProposals::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearStakingValidatorProposals::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearStakingValidatorProposals::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingValidatorProposals::AccountId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingValidatorProposals::PublicKey)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingValidatorProposals::Stake)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingValidatorProposals::EpochId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingValidatorProposals::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_staking_pool_rewards
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearStakingPoolRewards::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearStakingPoolRewards::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolRewards::PoolId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolRewards::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolRewards::RewardAmount)
                            .text()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolRewards::EventIndex)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolRewards::RawLog)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolRewards::ExecutionOutcomeId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolRewards::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_staking_pool_balances
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearStakingPoolBalances::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearStakingPoolBalances::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolBalances::PoolId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolBalances::AccountId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolBalances::BalanceType)
                            .string_len(20)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolBalances::Balance)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolBalances::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolBalances::EventIndex)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolBalances::RawLog)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolBalances::ExecutionOutcomeId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingPoolBalances::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop staking tables
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearStakingPoolBalances::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearStakingPoolRewards::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearStakingValidatorProposals::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearStakingEpochs::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearStakingDelegators::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearStakingPoolEvents::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearStakingPoolActions::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearStakingPools::Table))
                    .to_owned(),
            )
            .await?;

        // Drop token tables
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearTokenMetadata::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearTokenTransfers::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearTokenBalances::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearTokenContracts::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearTokenEvents::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearTokenEventsRaw::Table))
                    .to_owned(),
            )
            .await?;

        // Drop account tables
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearAccountEvents::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearAccessKeys::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearAccountBalances::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearAccounts::Table))
                    .to_owned(),
            )
            .await?;

        // Drop raw data tables
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearSyncCursors::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearStateChanges::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearExecutionOutcomes::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearReceipts::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearTransactions::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearChunks::Table))
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table((Alias::new("near"), NearBlocks::Table))
                    .to_owned(),
            )
            .await?;

        // Drop schema
        let db = manager.get_connection();
        db.execute_unprepared("DROP SCHEMA IF EXISTS near CASCADE")
            .await?;

        Ok(())
    }
}

// RAW DATA ENUMS
#[derive(DeriveIden)]
enum NearBlocks {
    Table,
    Height,
    Hash,
    PrevHash,
    EpochId,
    NextEpochId,
    Author,
    Timestamp,
    TimestampNanosec,
    GasPrice,
    TotalSupply,
    ValidatorReward,
    ProtocolVersion,
    ChunksIncluded,
    BlockOrdinal,
    PrevHeight,
}

#[derive(DeriveIden)]
enum NearChunks {
    Table,
    ChunkHash,
    BlockHeight,
    ShardId,
    Author,
    HeightCreated,
    HeightIncluded,
    GasUsed,
    GasLimit,
    ValidatorReward,
}

#[derive(DeriveIden)]
enum NearTransactions {
    Table,
    Hash,
    BlockHeight,
    SignerId,
    ReceiverId,
    Nonce,
    PriorityFee,
    PublicKey,
    Signature,
    Actions,
    RawData,
}

#[derive(DeriveIden)]
enum NearReceipts {
    Table,
    ReceiptId,
    BlockHeight,
    PredecessorId,
    ReceiverId,
    Receipt,
    RawData,
}

#[derive(DeriveIden)]
enum NearExecutionOutcomes {
    Table,
    Id,
    BlockHeight,
    ReceiptId,
    Outcome,
    RawData,
}

#[derive(DeriveIden)]
enum NearStateChanges {
    Table,
    Id,
    BlockHeight,
    ShardId,
    StateChanges,
}

#[derive(DeriveIden)]
enum NearSyncCursors {
    Table,
    ModuleName,
    LastProcessedHeight,
    UpdatedAt,
}

// ACCOUNT ENUMS
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

// TOKEN ENUMS
#[derive(DeriveIden)]
enum NearTokenEventsRaw {
    Table,
    Id,
    BlockHeight,
    BlockTimestamp,
    ShardId,
    ReceiptId,
    ExecutionOutcomeId,
    PredecessorAccountId,
    ContractId,
    LogIndex,
    LogText,
    CreatedAt,
}

#[derive(DeriveIden)]
enum NearTokenEvents {
    Table,
    Id,
    RawEventId,
    BlockHeight,
    BlockTimestamp,
    ContractId,
    Standard,
    Version,
    EventType,
    EventData,
    AffectedAccounts,
}

#[derive(DeriveIden)]
enum NearTokenContracts {
    Table,
    ContractId,
    StandardsDetected,
    FtMetadata,
    NftMetadata,
    MtMetadata,
    FirstSeenBlock,
    LastActivityBlock,
    TotalEvents,
}

#[derive(DeriveIden)]
enum NearTokenBalances {
    Table,
    ContractId,
    TokenTypeId,
    TokenId,
    AccountId,
    Amount,
    Standard,
    LastUpdatedBlock,
    Metadata,
}

#[derive(DeriveIden)]
enum NearTokenTransfers {
    Table,
    Id,
    EventId,
    BlockHeight,
    BlockTimestamp,
    ContractId,
    Standard,
    TokenTypeId,
    TokenId,
    FromAccount,
    ToAccount,
    Amount,
    Memo,
}

#[derive(DeriveIden)]
enum NearTokenMetadata {
    Table,
    ContractId,
    TokenTypeId,
    TokenId,
    Standard,
    Metadata,
    LastUpdatedBlock,
}

// STAKING ENUMS
#[derive(DeriveIden)]
enum NearStakingPools {
    Table,
    PoolId,
    PoolType,
    DetectedMethod,
    FirstSeenHeight,
    LastActivityHeight,
    IsActive,
    Metadata,
}

#[derive(DeriveIden)]
enum NearStakingPoolActions {
    Table,
    Id,
    PoolId,
    MethodName,
    CallerAccountId,
    ArgsJson,
    ArgsBase64,
    AttachedDeposit,
    TransactionHash,
    ReceiptId,
    BlockHeight,
    Timestamp,
    Success,
    ActionType,
    Amount,
}

#[derive(DeriveIden)]
enum NearStakingPoolEvents {
    Table,
    Id,
    PoolId,
    EventStandard,
    EventType,
    EventData,
    RawLog,
    LogIndex,
    ExecutionOutcomeId,
    BlockHeight,
    Timestamp,
}

#[derive(DeriveIden)]
enum NearStakingDelegators {
    Table,
    Id,
    PoolId,
    DelegatorAccountId,
    FirstInteractionHeight,
    FirstInteractionType,
    LastInteractionHeight,
    LastInteractionType,
    InteractionCount,
}

#[derive(DeriveIden)]
enum NearStakingEpochs {
    Table,
    EpochId,
    EpochHeight,
    StartBlockHeight,
    EndBlockHeight,
    ProtocolVersion,
    TotalStake,
    TotalValidatorReward,
    ValidatorsInfo,
    ChunkProducersSettlement,
    EpochDurationBlocks,
}

#[derive(DeriveIden)]
enum NearStakingValidatorProposals {
    Table,
    Id,
    BlockHeight,
    AccountId,
    PublicKey,
    Stake,
    EpochId,
    Timestamp,
}

#[derive(DeriveIden)]
enum NearStakingPoolRewards {
    Table,
    Id,
    PoolId,
    BlockHeight,
    RewardAmount,
    EventIndex,
    RawLog,
    ExecutionOutcomeId,
    Timestamp,
}

#[derive(DeriveIden)]
enum NearStakingPoolBalances {
    Table,
    Id,
    PoolId,
    AccountId,
    BalanceType,
    Balance,
    BlockHeight,
    EventIndex,
    RawLog,
    ExecutionOutcomeId,
    Timestamp,
}
