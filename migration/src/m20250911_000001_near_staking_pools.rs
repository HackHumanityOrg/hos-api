use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // =================
        // NEAR STAKING POOLS TABLES
        // =================

        // near_staking_pools - Registry of identified staking pool contracts
        manager
            .create_table(
                Table::create()
                    .table(NearStakingPools::Table)
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

        // near_staking_pool_actions - Staking-specific function calls to pools
        manager
            .create_table(
                Table::create()
                    .table(NearStakingPoolActions::Table)
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
                    .to_owned(),
            )
            .await?;

        // near_staking_pool_events - Events emitted by staking pools
        manager
            .create_table(
                Table::create()
                    .table(NearStakingPoolEvents::Table)
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

        // near_staking_delegators - Track delegator relationships
        manager
            .create_table(
                Table::create()
                    .table(NearStakingDelegators::Table)
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

        // near_staking_epochs - Track epoch transitions and rewards (aligned with near_primitives::EpochInfo)
        manager
            .create_table(
                Table::create()
                    .table(NearStakingEpochs::Table)
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
                            .not_null(), // Sum of all validator stakes as string
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::TotalValidatorReward)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::ValidatorsInfo)
                            .json()
                            .null(), // Array of ValidatorStakeView structures
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::ChunkProducersSettlement)
                            .json()
                            .null(), // Chunk producer assignments and rewards
                    )
                    .col(
                        ColumnDef::new(NearStakingEpochs::EpochDurationBlocks)
                            .big_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // near_staking_validator_proposals - Block-level validator proposals
        manager
            .create_table(
                Table::create()
                    .table(NearStakingValidatorProposals::Table)
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

        // near_staking_pool_rewards - Reward distribution events from logs
        manager
            .create_table(
                Table::create()
                    .table(NearStakingPoolRewards::Table)
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
                            .null(), // Parsed from logs
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

        // near_staking_pool_balances - Balance change events from logs
        manager
            .create_table(
                Table::create()
                    .table(NearStakingPoolBalances::Table)
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
                            .not_null(), // "staked" or "unstaked"
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

        // =================
        // ENHANCE EXISTING TABLES
        // =================

        // Add columns to near_staking_pool_actions for parsed data
        manager
            .alter_table(
                Table::alter()
                    .table(NearStakingPoolActions::Table)
                    .add_column(
                        ColumnDef::new(NearStakingPoolActions::ActionType)
                            .string_len(20)
                            .null(), // deposit/stake/unstake/withdraw/ping
                    )
                    .add_column(
                        ColumnDef::new(NearStakingPoolActions::Amount).text().null(), // Parsed from args_json
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop tables in reverse order to handle foreign key constraints
        manager
            .drop_table(
                Table::drop()
                    .table(NearStakingPoolBalances::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .table(NearStakingPoolRewards::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .table(NearStakingValidatorProposals::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(NearStakingEpochs::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(NearStakingDelegators::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(NearStakingPoolEvents::Table).to_owned())
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .table(NearStakingPoolActions::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(Table::drop().table(NearStakingPools::Table).to_owned())
            .await?;

        Ok(())
    }
}

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
