use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table((Alias::new("near"), NearVenearLockupEvents::Table))
                    .add_column(
                        ColumnDef::new(NearVenearLockupEvents::LockupTimestampNs)
                            .big_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearVenearGovernanceActions::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::ActionSource)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::ActionIndex)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::TransactionHash).string_len(64),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::ReceiptId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::ExecutionOutcomeId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::BlockTimestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::ShardId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::PredecessorAccountId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::ReceiverAccountId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::ContractId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::MethodName)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearVenearGovernanceActions::ArgsJson).json())
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::ArgsBase64)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::AttachedDeposit)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::Success)
                            .boolean()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearVenearGovernanceActions::SuccessValueJson).json())
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::ActionCategory)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearVenearGovernanceActions::ProposalId).big_integer())
                    .col(ColumnDef::new(NearVenearGovernanceActions::VoteOption).integer())
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::DelegatorAccountId)
                            .string_len(128),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::DelegateeAccountId)
                            .string_len(128),
                    )
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::OwnerAccountId).string_len(128),
                    )
                    .col(ColumnDef::new(NearVenearGovernanceActions::Metadata).json())
                    .col(
                        ColumnDef::new(NearVenearGovernanceActions::CreatedAt)
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
                    .name("ux_near_venear_governance_actions_identity")
                    .table((Alias::new("near"), NearVenearGovernanceActions::Table))
                    .col(NearVenearGovernanceActions::ReceiptId)
                    .col(NearVenearGovernanceActions::ActionIndex)
                    .col(NearVenearGovernanceActions::MethodName)
                    .col(NearVenearGovernanceActions::ContractId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_near_venear_governance_actions_contract_height")
                    .table((Alias::new("near"), NearVenearGovernanceActions::Table))
                    .col(NearVenearGovernanceActions::ContractId)
                    .col(NearVenearGovernanceActions::BlockHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_near_venear_governance_actions_method_height")
                    .table((Alias::new("near"), NearVenearGovernanceActions::Table))
                    .col(NearVenearGovernanceActions::MethodName)
                    .col(NearVenearGovernanceActions::BlockHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_near_venear_governance_actions_category_height")
                    .table((Alias::new("near"), NearVenearGovernanceActions::Table))
                    .col(NearVenearGovernanceActions::ActionCategory)
                    .col(NearVenearGovernanceActions::BlockHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_near_venear_governance_actions_proposal_height")
                    .table((Alias::new("near"), NearVenearGovernanceActions::Table))
                    .col(NearVenearGovernanceActions::ProposalId)
                    .col(NearVenearGovernanceActions::BlockHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearVenearUnhandledEvents::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::BlockTimestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::ShardId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::ReceiptId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::ExecutionOutcomeId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::PredecessorAccountId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::ContractId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::Success)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::Standard)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::EventType)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::LogIndex)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::DataIndex)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearVenearUnhandledEvents::EventData).json())
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::LogText)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::Reason)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearUnhandledEvents::CreatedAt)
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
                    .name("ux_near_venear_unhandled_events_identity")
                    .table((Alias::new("near"), NearVenearUnhandledEvents::Table))
                    .col(NearVenearUnhandledEvents::ExecutionOutcomeId)
                    .col(NearVenearUnhandledEvents::LogIndex)
                    .col(NearVenearUnhandledEvents::DataIndex)
                    .col(NearVenearUnhandledEvents::ContractId)
                    .col(NearVenearUnhandledEvents::Standard)
                    .col(NearVenearUnhandledEvents::EventType)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_near_venear_unhandled_events_contract_height")
                    .table((Alias::new("near"), NearVenearUnhandledEvents::Table))
                    .col(NearVenearUnhandledEvents::ContractId)
                    .col(NearVenearUnhandledEvents::BlockHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_near_venear_unhandled_events_event_height")
                    .table((Alias::new("near"), NearVenearUnhandledEvents::Table))
                    .col(NearVenearUnhandledEvents::Standard)
                    .col(NearVenearUnhandledEvents::EventType)
                    .col(NearVenearUnhandledEvents::BlockHeight)
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
                    .table((Alias::new("near"), NearVenearUnhandledEvents::Table))
                    .to_owned(),
            )
            .await?;

        manager
            .drop_table(
                Table::drop()
                    .if_exists()
                    .table((Alias::new("near"), NearVenearGovernanceActions::Table))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table((Alias::new("near"), NearVenearLockupEvents::Table))
                    .drop_column(NearVenearLockupEvents::LockupTimestampNs)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum NearVenearLockupEvents {
    Table,
    LockupTimestampNs,
}

#[derive(DeriveIden)]
enum NearVenearGovernanceActions {
    Table,
    Id,
    ActionSource,
    ActionIndex,
    TransactionHash,
    ReceiptId,
    ExecutionOutcomeId,
    BlockHeight,
    BlockTimestamp,
    ShardId,
    PredecessorAccountId,
    ReceiverAccountId,
    ContractId,
    MethodName,
    ArgsJson,
    ArgsBase64,
    AttachedDeposit,
    Success,
    SuccessValueJson,
    ActionCategory,
    ProposalId,
    VoteOption,
    DelegatorAccountId,
    DelegateeAccountId,
    OwnerAccountId,
    Metadata,
    CreatedAt,
}

#[derive(DeriveIden)]
enum NearVenearUnhandledEvents {
    Table,
    Id,
    BlockHeight,
    BlockTimestamp,
    ShardId,
    ReceiptId,
    ExecutionOutcomeId,
    PredecessorAccountId,
    ContractId,
    Success,
    Standard,
    EventType,
    LogIndex,
    DataIndex,
    EventData,
    LogText,
    Reason,
    CreatedAt,
}
