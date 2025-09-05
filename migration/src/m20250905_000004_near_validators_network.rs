use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // =================
        // VALIDATOR TABLES
        // =================

        // near_validators - All validator accounts we've seen
        manager
            .create_table(
                Table::create()
                    .table(NearValidators::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearValidators::AccountId)
                            .string_len(128)
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(NearValidators::PublicKey).string_len(128))
                    .col(
                        ColumnDef::new(NearValidators::FirstSeenHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidators::FirstSeenTimestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearValidators::LastSeenHeight).big_integer())
                    .col(ColumnDef::new(NearValidators::LastSeenTimestamp).big_integer())
                    .to_owned(),
            )
            .await?;

        // near_validator_proposals - All validator proposals from block headers
        manager
            .create_table(
                Table::create()
                    .table(NearValidatorProposals::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearValidatorProposals::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorProposals::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorProposals::AccountId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorProposals::PublicKey)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorProposals::Stake)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorProposals::EpochId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorProposals::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorProposals::Source)
                            .string_len(16)
                            .not_null(), // 'block' or 'chunk'
                    )
                    .col(
                        ColumnDef::new(NearValidatorProposals::ShardId).integer(), // null for block-level proposals
                    )
                    .to_owned(),
            )
            .await?;

        // near_block_producers - Who produced each block
        manager
            .create_table(
                Table::create()
                    .table(NearBlockProducers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearBlockProducers::BlockHeight)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearBlockProducers::ProducerId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearBlockProducers::EpochId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearBlockProducers::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearBlockProducers::GasPrice)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_chunk_producers - Who produced each chunk
        manager
            .create_table(
                Table::create()
                    .table(NearChunkProducers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearChunkProducers::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearChunkProducers::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearChunkProducers::ShardId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearChunkProducers::ProducerId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearChunkProducers::EpochId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearChunkProducers::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_validator_approvals - Approval counts per block
        manager
            .create_table(
                Table::create()
                    .table(NearValidatorApprovals::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearValidatorApprovals::BlockHeight)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorApprovals::TotalApprovals)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorApprovals::TotalSlots)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorApprovals::EpochId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorApprovals::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // =================
        // NETWORK TABLES
        // =================

        // near_epochs - Track epoch transitions
        manager
            .create_table(
                Table::create()
                    .table(NearEpochs::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearEpochs::EpochId)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearEpochs::StartHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearEpochs::StartTimestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearEpochs::EndHeight).big_integer())
                    .col(ColumnDef::new(NearEpochs::EndTimestamp).big_integer())
                    .col(
                        ColumnDef::new(NearEpochs::ProtocolVersion)
                            .integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_network_state - Gas price, protocol version, total supply per block
        manager
            .create_table(
                Table::create()
                    .table(NearNetworkState::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearNetworkState::BlockHeight)
                            .big_integer()
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::EpochId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::GasPrice)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::ProtocolVersion)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::TotalSupply)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // =================
        // INDEXES
        // =================

        // near_validators indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_near_validators_first_seen_height")
                    .table(NearValidators::Table)
                    .col(NearValidators::FirstSeenHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_validators_last_seen_height")
                    .table(NearValidators::Table)
                    .col(NearValidators::LastSeenHeight)
                    .to_owned(),
            )
            .await?;

        // near_validator_proposals indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_near_validator_proposals_block_height")
                    .table(NearValidatorProposals::Table)
                    .col(NearValidatorProposals::BlockHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_validator_proposals_account_id")
                    .table(NearValidatorProposals::Table)
                    .col(NearValidatorProposals::AccountId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_validator_proposals_epoch_id")
                    .table(NearValidatorProposals::Table)
                    .col(NearValidatorProposals::EpochId)
                    .to_owned(),
            )
            .await?;

        // near_block_producers indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_near_block_producers_producer_id")
                    .table(NearBlockProducers::Table)
                    .col(NearBlockProducers::ProducerId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_block_producers_epoch_id")
                    .table(NearBlockProducers::Table)
                    .col(NearBlockProducers::EpochId)
                    .to_owned(),
            )
            .await?;

        // near_chunk_producers indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_near_chunk_producers_block_shard")
                    .table(NearChunkProducers::Table)
                    .col(NearChunkProducers::BlockHeight)
                    .col(NearChunkProducers::ShardId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_chunk_producers_producer_id")
                    .table(NearChunkProducers::Table)
                    .col(NearChunkProducers::ProducerId)
                    .to_owned(),
            )
            .await?;

        // near_validator_approvals indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_near_validator_approvals_epoch_id")
                    .table(NearValidatorApprovals::Table)
                    .col(NearValidatorApprovals::EpochId)
                    .to_owned(),
            )
            .await?;

        // near_epochs indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_near_epochs_start_height")
                    .table(NearEpochs::Table)
                    .col(NearEpochs::StartHeight)
                    .to_owned(),
            )
            .await?;

        // near_network_state indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_near_network_state_epoch_id")
                    .table(NearNetworkState::Table)
                    .col(NearNetworkState::EpochId)
                    .to_owned(),
            )
            .await?;

        // =================
        // ENHANCED VALIDATOR TABLES
        // =================

        // near_validator_kickouts - Track validator kickouts and slashing
        manager
            .create_table(
                Table::create()
                    .table(NearValidatorKickouts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearValidatorKickouts::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorKickouts::ValidatorId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorKickouts::EpochId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorKickouts::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorKickouts::Reason)
                            .string_len(64)
                            .not_null(), // 'NotEnoughBlocks', 'NotEnoughStake', 'Unstaked', etc.
                    )
                    .col(
                        ColumnDef::new(NearValidatorKickouts::StakeReturned)
                            .decimal_len(39, 0)
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorKickouts::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_validator_rewards - Track validator rewards per epoch
        manager
            .create_table(
                Table::create()
                    .table(NearValidatorRewards::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearValidatorRewards::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorRewards::ValidatorId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorRewards::EpochId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorRewards::RewardAmount)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorRewards::DelegatorRewards)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorRewards::CommissionEarned)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorRewards::CommissionRate)
                            .decimal_len(5, 2)
                            .not_null(), // 0.00 - 100.00 percentage
                    )
                    .col(
                        ColumnDef::new(NearValidatorRewards::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_validator_seat_prices - Track minimum stake required per epoch
        manager
            .create_table(
                Table::create()
                    .table(NearValidatorSeatPrices::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearValidatorSeatPrices::EpochId)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorSeatPrices::SeatPrice)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorSeatPrices::TotalStake)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorSeatPrices::ValidatorCount)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorSeatPrices::StartHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorSeatPrices::StartTimestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // =================
        // ENHANCED INDEXES
        // =================

        // near_validator_kickouts indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_near_validator_kickouts_validator")
                    .table(NearValidatorKickouts::Table)
                    .col(NearValidatorKickouts::ValidatorId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_validator_kickouts_epoch")
                    .table(NearValidatorKickouts::Table)
                    .col(NearValidatorKickouts::EpochId)
                    .to_owned(),
            )
            .await?;

        // near_validator_rewards indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_near_validator_rewards_validator_epoch")
                    .table(NearValidatorRewards::Table)
                    .col(NearValidatorRewards::ValidatorId)
                    .col(NearValidatorRewards::EpochId)
                    .to_owned(),
            )
            .await?;

        // near_validator_seat_prices indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_near_validator_seat_prices_start_height")
                    .table(NearValidatorSeatPrices::Table)
                    .col(NearValidatorSeatPrices::StartHeight)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop enhanced tables first (reverse order)
        manager
            .drop_table(
                Table::drop()
                    .table(NearValidatorSeatPrices::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(NearValidatorRewards::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(NearValidatorKickouts::Table).to_owned())
            .await?;

        // Drop original tables
        manager
            .drop_table(Table::drop().table(NearNetworkState::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(NearEpochs::Table).to_owned())
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .table(NearValidatorApprovals::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(NearChunkProducers::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(NearBlockProducers::Table).to_owned())
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .table(NearValidatorProposals::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(NearValidators::Table).to_owned())
            .await?;

        Ok(())
    }
}

// =================
// TABLE ENUMS
// =================

#[derive(DeriveIden)]
enum NearValidators {
    Table,
    AccountId,
    PublicKey,
    FirstSeenHeight,
    FirstSeenTimestamp,
    LastSeenHeight,
    LastSeenTimestamp,
}

#[derive(DeriveIden)]
enum NearValidatorProposals {
    Table,
    Id,
    BlockHeight,
    AccountId,
    PublicKey,
    Stake,
    EpochId,
    Timestamp,
    Source,
    ShardId,
}

#[derive(DeriveIden)]
enum NearBlockProducers {
    Table,
    BlockHeight,
    ProducerId,
    EpochId,
    Timestamp,
    GasPrice,
}

#[derive(DeriveIden)]
enum NearChunkProducers {
    Table,
    Id,
    BlockHeight,
    ShardId,
    ProducerId,
    EpochId,
    Timestamp,
}

#[derive(DeriveIden)]
enum NearValidatorApprovals {
    Table,
    BlockHeight,
    TotalApprovals,
    TotalSlots,
    EpochId,
    Timestamp,
}

#[derive(DeriveIden)]
enum NearEpochs {
    Table,
    EpochId,
    StartHeight,
    StartTimestamp,
    EndHeight,
    EndTimestamp,
    ProtocolVersion,
}

#[derive(DeriveIden)]
enum NearNetworkState {
    Table,
    BlockHeight,
    EpochId,
    GasPrice,
    ProtocolVersion,
    TotalSupply,
    Timestamp,
}

// =================
// ENHANCED TABLE ENUMS
// =================

#[derive(DeriveIden)]
enum NearValidatorKickouts {
    Table,
    Id,
    ValidatorId,
    EpochId,
    BlockHeight,
    Reason,
    StakeReturned,
    Timestamp,
}

#[derive(DeriveIden)]
enum NearValidatorRewards {
    Table,
    Id,
    ValidatorId,
    EpochId,
    RewardAmount,
    DelegatorRewards,
    CommissionEarned,
    CommissionRate,
    Timestamp,
}

#[derive(DeriveIden)]
enum NearValidatorSeatPrices {
    Table,
    EpochId,
    SeatPrice,
    TotalStake,
    ValidatorCount,
    StartHeight,
    StartTimestamp,
}
