use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // =================
        // RAW NEAR BLOCKCHAIN DATA TABLES
        // =================

        // near_blocks - Raw block data from BlockView
        manager
            .create_table(
                Table::create()
                    .table(NearBlocks::Table)
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
                    .col(
                        ColumnDef::new(NearBlocks::GasPrice).text().not_null(), // Store Balance as string to preserve precision
                    )
                    .col(
                        ColumnDef::new(NearBlocks::TotalSupply).text().not_null(), // Store Balance as string to preserve precision
                    )
                    .col(
                        ColumnDef::new(NearBlocks::ValidatorReward)
                            .text()
                            .not_null(), // Store Balance as string to preserve precision
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
                    .col(
                        ColumnDef::new(NearBlocks::RawData).json().null(), // Store complete BlockView as JSON
                    )
                    .to_owned(),
            )
            .await?;

        // near_chunks - Raw chunk data from ChunkHeaderView
        manager
            .create_table(
                Table::create()
                    .table(NearChunks::Table)
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
                            .not_null(), // Store Balance as string to preserve precision
                    )
                    .col(
                        ColumnDef::new(NearChunks::RawData).json().null(), // Store complete IndexerChunkView as JSON
                    )
                    .to_owned(),
            )
            .await?;

        // near_transactions - Raw transaction data from SignedTransactionView
        manager
            .create_table(
                Table::create()
                    .table(NearTransactions::Table)
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
                    .col(
                        ColumnDef::new(NearTransactions::SignerId)
                            .text()
                            .not_null(),
                    )
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
                    .col(
                        ColumnDef::new(NearTransactions::Actions).json().not_null(), // Store ActionView array as JSON
                    )
                    .col(
                        ColumnDef::new(NearTransactions::RawData).json().null(), // Store complete SignedTransactionView as JSON
                    )
                    .to_owned(),
            )
            .await?;

        // near_receipts - Raw receipt data from ReceiptView
        manager
            .create_table(
                Table::create()
                    .table(NearReceipts::Table)
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
                    .col(
                        ColumnDef::new(NearReceipts::ReceiverId)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearReceipts::Receipt).json().not_null(), // Store ReceiptEnumView as JSON
                    )
                    .col(
                        ColumnDef::new(NearReceipts::RawData).json().null(), // Store complete ReceiptView as JSON
                    )
                    .to_owned(),
            )
            .await?;

        // near_execution_outcomes - Raw execution outcome data
        manager
            .create_table(
                Table::create()
                    .table(NearExecutionOutcomes::Table)
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
                            .not_null(), // Store ExecutionOutcomeView as JSON
                    )
                    .col(
                        ColumnDef::new(NearExecutionOutcomes::RawData).json().null(), // Store complete ExecutionOutcomeWithIdView as JSON
                    )
                    .to_owned(),
            )
            .await?;

        // near_state_changes - Raw state change data
        manager
            .create_table(
                Table::create()
                    .table(NearStateChanges::Table)
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
                            .not_null(), // Store StateChangesView as JSON
                    )
                    .to_owned(),
            )
            .await?;

        // near_sync_cursors - Cursor tracking table
        manager
            .create_table(
                Table::create()
                    .table(NearSyncCursors::Table)
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

        // =================
        // INDICES - Commented out to avoid conflicts with existing tables
        // =================
        // Indices can be added later if needed after tables are created successfully

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop tables in reverse order
        manager
            .drop_table(Table::drop().table(NearSyncCursors::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearStateChanges::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearExecutionOutcomes::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearReceipts::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearTransactions::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearChunks::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearBlocks::Table).to_owned())
            .await?;
        Ok(())
    }
}

/// Learn more at https://docs.rs/sea-query#iden
#[derive(DeriveIden)]
enum NearBlocks {
    Table,
    Height,
    Hash,
    PrevHash,
    EpochId,
    NextEpochId,
    Timestamp,
    TimestampNanosec,
    GasPrice,
    TotalSupply,
    ValidatorReward,
    ProtocolVersion,
    ChunksIncluded,
    BlockOrdinal,
    PrevHeight,
    RawData,
}

#[derive(DeriveIden)]
enum NearChunks {
    Table,
    ChunkHash,
    BlockHeight,
    ShardId,
    HeightCreated,
    HeightIncluded,
    GasUsed,
    GasLimit,
    ValidatorReward,
    RawData,
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
