use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table((Alias::new("near"), NearVenearProposalEvents::Table))
                    .add_column(
                        ColumnDef::new(NearVenearProposalEvents::TransactionHash).string_len(64),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table((Alias::new("near"), NearVenearVotes::Table))
                    .add_column(ColumnDef::new(NearVenearVotes::TransactionHash).string_len(64))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table((
                        Alias::new("near"),
                        NearVenearAccountVotingPowerEvents::Table,
                    ))
                    .add_column(
                        ColumnDef::new(NearVenearAccountVotingPowerEvents::TransactionHash)
                            .string_len(64),
                    )
                    .to_owned(),
            )
            .await?;

        manager
            .create_table(
                Table::create()
                    .table((Alias::new("near"), NearVenearReceiptTxOrigins::Table))
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearVenearReceiptTxOrigins::ReceiptId)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearVenearReceiptTxOrigins::TransactionHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearReceiptTxOrigins::FirstObservedHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearReceiptTxOrigins::LastObservedHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearVenearReceiptTxOrigins::CreatedAt)
                            .timestamp()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(NearVenearReceiptTxOrigins::UpdatedAt)
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
                    .name("ix_near_venear_receipt_tx_origins_tx_hash")
                    .table((Alias::new("near"), NearVenearReceiptTxOrigins::Table))
                    .col(NearVenearReceiptTxOrigins::TransactionHash)
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
                    .table((Alias::new("near"), NearVenearReceiptTxOrigins::Table))
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table((
                        Alias::new("near"),
                        NearVenearAccountVotingPowerEvents::Table,
                    ))
                    .drop_column(NearVenearAccountVotingPowerEvents::TransactionHash)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table((Alias::new("near"), NearVenearVotes::Table))
                    .drop_column(NearVenearVotes::TransactionHash)
                    .to_owned(),
            )
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table((Alias::new("near"), NearVenearProposalEvents::Table))
                    .drop_column(NearVenearProposalEvents::TransactionHash)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum NearVenearProposalEvents {
    Table,
    TransactionHash,
}

#[derive(DeriveIden)]
enum NearVenearVotes {
    Table,
    TransactionHash,
}

#[derive(DeriveIden)]
enum NearVenearAccountVotingPowerEvents {
    Table,
    TransactionHash,
}

#[derive(DeriveIden)]
enum NearVenearReceiptTxOrigins {
    Table,
    ReceiptId,
    TransactionHash,
    FirstObservedHeight,
    LastObservedHeight,
    CreatedAt,
    UpdatedAt,
}
