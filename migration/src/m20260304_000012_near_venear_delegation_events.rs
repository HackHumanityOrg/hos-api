use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearVenearDelegationEvents::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::DelegatorAccountId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::DelegateeAccountId)
                            .string_len(128),
                    )
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::EventType)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::ActionSource)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::ActionIndex)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearVenearDelegationEvents::ArgsJson).json())
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::ArgsBase64)
                            .text()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::AttachedDeposit)
                            .text()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearVenearDelegationEvents::TransactionHash).string_len(64))
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::ReceiptId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::ExecutionOutcomeId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::Success)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::BlockTimestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::ShardId)
                            .integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::ContractId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearVenearDelegationEvents::Metadata).json())
                    .col(
                        ColumnDef::new(NearVenearDelegationEvents::CreatedAt)
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
                    .name("ux_near_venear_delegation_events_identity")
                    .table((Alias::new("near"), NearVenearDelegationEvents::Table))
                    .col(NearVenearDelegationEvents::ReceiptId)
                    .col(NearVenearDelegationEvents::ActionIndex)
                    .col(NearVenearDelegationEvents::EventType)
                    .col(NearVenearDelegationEvents::DelegatorAccountId)
                    .col(NearVenearDelegationEvents::ContractId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_near_venear_delegation_events_delegator_height")
                    .table((Alias::new("near"), NearVenearDelegationEvents::Table))
                    .col(NearVenearDelegationEvents::DelegatorAccountId)
                    .col(NearVenearDelegationEvents::BlockHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_near_venear_delegation_events_delegatee_height")
                    .table((Alias::new("near"), NearVenearDelegationEvents::Table))
                    .col(NearVenearDelegationEvents::DelegateeAccountId)
                    .col(NearVenearDelegationEvents::BlockHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ix_near_venear_delegation_events_event_height")
                    .table((Alias::new("near"), NearVenearDelegationEvents::Table))
                    .col(NearVenearDelegationEvents::EventType)
                    .col(NearVenearDelegationEvents::BlockHeight)
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
                    .table((Alias::new("near"), NearVenearDelegationEvents::Table))
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum NearVenearDelegationEvents {
    Table,
    Id,
    DelegatorAccountId,
    DelegateeAccountId,
    EventType,
    ActionSource,
    ActionIndex,
    ArgsJson,
    ArgsBase64,
    AttachedDeposit,
    TransactionHash,
    ReceiptId,
    ExecutionOutcomeId,
    Success,
    BlockHeight,
    BlockTimestamp,
    ShardId,
    ContractId,
    Metadata,
    CreatedAt,
}
