use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table((Alias::new("near"), NearVenearEventsRaw::Table))
                    .add_column(
                        ColumnDef::new(NearVenearEventsRaw::BlockHash)
                            .string_len(64)
                            .not_null()
                            .default(""),
                    )
                    .add_column(ColumnDef::new(NearVenearEventsRaw::ChunkHash).string_len(64))
                    .add_column(
                        ColumnDef::new(NearVenearEventsRaw::SignerAccountId).string_len(128),
                    )
                    .add_column(ColumnDef::new(NearVenearEventsRaw::SignerPublicKey).text())
                    .add_column(ColumnDef::new(NearVenearEventsRaw::GasPrice).text())
                    .add_column(ColumnDef::new(NearVenearEventsRaw::ActionGas).big_integer())
                    .add_column(ColumnDef::new(NearVenearEventsRaw::SuccessValueBase64).text())
                    .add_column(
                        ColumnDef::new(NearVenearEventsRaw::OutcomeStatus)
                            .string_len(32)
                            .not_null()
                            .default("Unknown"),
                    )
                    .add_column(ColumnDef::new(NearVenearEventsRaw::OutcomeReceiptIds).json())
                    .add_column(ColumnDef::new(NearVenearEventsRaw::OutcomeLogs).json())
                    .add_column(ColumnDef::new(NearVenearEventsRaw::OutcomeResultsJson).json())
                    .add_column(
                        ColumnDef::new(NearVenearEventsRaw::ExecutorAccountId).string_len(128),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table((Alias::new("near"), NearVenearEventsRaw::Table))
                    .drop_column(NearVenearEventsRaw::ExecutorAccountId)
                    .drop_column(NearVenearEventsRaw::OutcomeResultsJson)
                    .drop_column(NearVenearEventsRaw::OutcomeLogs)
                    .drop_column(NearVenearEventsRaw::OutcomeReceiptIds)
                    .drop_column(NearVenearEventsRaw::OutcomeStatus)
                    .drop_column(NearVenearEventsRaw::SuccessValueBase64)
                    .drop_column(NearVenearEventsRaw::ActionGas)
                    .drop_column(NearVenearEventsRaw::GasPrice)
                    .drop_column(NearVenearEventsRaw::SignerPublicKey)
                    .drop_column(NearVenearEventsRaw::SignerAccountId)
                    .drop_column(NearVenearEventsRaw::ChunkHash)
                    .drop_column(NearVenearEventsRaw::BlockHash)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum NearVenearEventsRaw {
    Table,
    BlockHash,
    ChunkHash,
    SignerAccountId,
    SignerPublicKey,
    GasPrice,
    ActionGas,
    SuccessValueBase64,
    OutcomeStatus,
    OutcomeReceiptIds,
    OutcomeLogs,
    OutcomeResultsJson,
    ExecutorAccountId,
}
