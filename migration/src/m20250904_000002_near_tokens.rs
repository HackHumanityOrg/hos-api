use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // near_tokens - Token contract registry for NEP-141 and NEP-171
        manager
            .create_table(
                Table::create()
                    .table(NearTokens::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearTokens::ContractId)
                            .string_len(128)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearTokens::TokenStandard)
                            .string_len(16)
                            .not_null(), // "nep141" or "nep171"
                    )
                    .col(ColumnDef::new(NearTokens::Name).string_len(256))
                    .col(ColumnDef::new(NearTokens::Symbol).string_len(64))
                    .col(ColumnDef::new(NearTokens::Decimals).integer())
                    .col(ColumnDef::new(NearTokens::TotalSupply).decimal_len(39, 0))
                    .col(ColumnDef::new(NearTokens::Icon).text())
                    .col(ColumnDef::new(NearTokens::Reference).text())
                    .col(ColumnDef::new(NearTokens::ReferenceHash).string_len(100))
                    .col(ColumnDef::new(NearTokens::BaseUri).text())
                    .col(ColumnDef::new(NearTokens::FirstSeenHeight).big_integer())
                    .col(ColumnDef::new(NearTokens::FirstSeenTime).big_integer())
                    .col(ColumnDef::new(NearTokens::LastSeenHeight).big_integer())
                    .col(ColumnDef::new(NearTokens::LastSeenTime).big_integer())
                    .to_owned(),
            )
            .await?;

        // Indexes for near_tokens
        manager
            .create_index(
                Index::create()
                    .name("idx_near_tokens_standard")
                    .table(NearTokens::Table)
                    .col(NearTokens::TokenStandard)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_tokens_symbol")
                    .table(NearTokens::Table)
                    .col(NearTokens::Symbol)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_tokens_first_seen_height")
                    .table(NearTokens::Table)
                    .col(NearTokens::FirstSeenHeight)
                    .to_owned(),
            )
            .await?;

        // near_token_transfers - All token transfer events (NEP-141 and NEP-171)
        manager
            .create_table(
                Table::create()
                    .table(NearTokenTransfers::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearTokenTransfers::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearTokenTransfers::ContractId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenTransfers::TokenStandard)
                            .string_len(16)
                            .not_null(), // "nep141" or "nep171"
                    )
                    .col(
                        ColumnDef::new(NearTokenTransfers::EventType)
                            .string_len(32)
                            .not_null(), // "ft_mint", "ft_transfer", "ft_burn", "nft_mint", "nft_transfer", "nft_burn"
                    )
                    .col(ColumnDef::new(NearTokenTransfers::FromAccountId).string_len(128)) // NULL for mint
                    .col(ColumnDef::new(NearTokenTransfers::ToAccountId).string_len(128)) // NULL for burn
                    .col(ColumnDef::new(NearTokenTransfers::Amount).decimal_len(39, 0)) // For NEP-141 transfers
                    .col(ColumnDef::new(NearTokenTransfers::TokenIds).text()) // JSON array for NEP-171
                    .col(ColumnDef::new(NearTokenTransfers::Memo).text())
                    .col(ColumnDef::new(NearTokenTransfers::AuthorizedId).string_len(128)) // For NFT burns
                    .col(
                        ColumnDef::new(NearTokenTransfers::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenTransfers::BlockTime)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenTransfers::TxHash)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenTransfers::ReceiptHash)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenTransfers::LogIndex)
                            .integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        // FK: near_token_transfers.contract_id -> near_tokens(contract_id)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_token_transfers_contract")
                    .from(NearTokenTransfers::Table, NearTokenTransfers::ContractId)
                    .to(NearTokens::Table, NearTokens::ContractId)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // Indexes for near_token_transfers
        manager
            .create_index(
                Index::create()
                    .name("idx_near_token_transfers_contract")
                    .table(NearTokenTransfers::Table)
                    .col(NearTokenTransfers::ContractId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_token_transfers_from_account")
                    .table(NearTokenTransfers::Table)
                    .col(NearTokenTransfers::FromAccountId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_token_transfers_to_account")
                    .table(NearTokenTransfers::Table)
                    .col(NearTokenTransfers::ToAccountId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_token_transfers_event_type")
                    .table(NearTokenTransfers::Table)
                    .col(NearTokenTransfers::EventType)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_token_transfers_standard")
                    .table(NearTokenTransfers::Table)
                    .col(NearTokenTransfers::TokenStandard)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_token_transfers_block_height")
                    .table(NearTokenTransfers::Table)
                    .col(NearTokenTransfers::BlockHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_token_transfers_block_time")
                    .table(NearTokenTransfers::Table)
                    .col(NearTokenTransfers::BlockTime)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_token_transfers_tx_hash")
                    .table(NearTokenTransfers::Table)
                    .col(NearTokenTransfers::TxHash)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_token_transfers_receipt_hash")
                    .table(NearTokenTransfers::Table)
                    .col(NearTokenTransfers::ReceiptHash)
                    .to_owned(),
            )
            .await?;

        // Unique constraint to prevent duplicate event logs
        manager
            .create_index(
                Index::create()
                    .name("ux_near_token_transfers_dedup")
                    .table(NearTokenTransfers::Table)
                    .col(NearTokenTransfers::ReceiptHash)
                    .col(NearTokenTransfers::LogIndex)
                    .col(NearTokenTransfers::EventType)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Composite indexes for common query patterns
        manager
            .create_index(
                Index::create()
                    .name("idx_near_token_transfers_contract_event")
                    .table(NearTokenTransfers::Table)
                    .col(NearTokenTransfers::ContractId)
                    .col(NearTokenTransfers::EventType)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_token_transfers_contract_time")
                    .table(NearTokenTransfers::Table)
                    .col(NearTokenTransfers::ContractId)
                    .col(NearTokenTransfers::BlockTime)
                    .to_owned(),
            )
            .await?;

        // near_nfts - Individual NFT metadata and ownership
        manager
            .create_table(
                Table::create()
                    .table(NearNfts::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearNfts::Id)
                            .string_len(256)
                            .not_null()
                            .primary_key(), // Format: "{contract_id}:{token_id}"
                    )
                    .col(
                        ColumnDef::new(NearNfts::ContractId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearNfts::TokenId).string_len(128).not_null())
                    .col(ColumnDef::new(NearNfts::OwnerId).string_len(128))
                    .col(ColumnDef::new(NearNfts::ApprovedAccountIds).text()) // JSON array
                    .col(ColumnDef::new(NearNfts::MetadataTitle).string_len(512))
                    .col(ColumnDef::new(NearNfts::MetadataDescription).text())
                    .col(ColumnDef::new(NearNfts::MetadataMedia).text())
                    .col(ColumnDef::new(NearNfts::MetadataMediaHash).string_len(100))
                    .col(ColumnDef::new(NearNfts::MetadataCopies).big_integer())
                    .col(ColumnDef::new(NearNfts::MetadataIssuedAt).big_integer())
                    .col(ColumnDef::new(NearNfts::MetadataExpiresAt).big_integer())
                    .col(ColumnDef::new(NearNfts::MetadataStartsAt).big_integer())
                    .col(ColumnDef::new(NearNfts::MetadataUpdatedAt).big_integer())
                    .col(ColumnDef::new(NearNfts::MetadataExtra).text()) // JSON for extra metadata
                    .col(ColumnDef::new(NearNfts::MetadataReference).text())
                    .col(ColumnDef::new(NearNfts::MetadataReferenceHash).string_len(100))
                    .col(ColumnDef::new(NearNfts::MintedHeight).big_integer())
                    .col(ColumnDef::new(NearNfts::MintedTime).big_integer())
                    .col(ColumnDef::new(NearNfts::MintedByTxHash).string_len(100))
                    .col(ColumnDef::new(NearNfts::MintedByReceiptHash).string_len(100))
                    .col(ColumnDef::new(NearNfts::LastTransferHeight).big_integer())
                    .col(ColumnDef::new(NearNfts::LastTransferTime).big_integer())
                    .col(
                        ColumnDef::new(NearNfts::Burned)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(NearNfts::BurnedHeight).big_integer())
                    .col(ColumnDef::new(NearNfts::BurnedTime).big_integer())
                    .to_owned(),
            )
            .await?;

        // FK: near_nfts.contract_id -> near_tokens(contract_id)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_nfts_contract")
                    .from(NearNfts::Table, NearNfts::ContractId)
                    .to(NearTokens::Table, NearTokens::ContractId)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // Indexes for near_nfts
        manager
            .create_index(
                Index::create()
                    .name("idx_near_nfts_contract")
                    .table(NearNfts::Table)
                    .col(NearNfts::ContractId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_nfts_owner")
                    .table(NearNfts::Table)
                    .col(NearNfts::OwnerId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_nfts_burned")
                    .table(NearNfts::Table)
                    .col(NearNfts::Burned)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_nfts_minted_height")
                    .table(NearNfts::Table)
                    .col(NearNfts::MintedHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_nfts_minted_time")
                    .table(NearNfts::Table)
                    .col(NearNfts::MintedTime)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_nfts_last_transfer_time")
                    .table(NearNfts::Table)
                    .col(NearNfts::LastTransferTime)
                    .to_owned(),
            )
            .await?;

        // Unique constraint on contract_id + token_id
        manager
            .create_index(
                Index::create()
                    .name("ux_near_nfts_contract_token")
                    .table(NearNfts::Table)
                    .col(NearNfts::ContractId)
                    .col(NearNfts::TokenId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Composite indexes for common queries
        manager
            .create_index(
                Index::create()
                    .name("idx_near_nfts_owner_contract")
                    .table(NearNfts::Table)
                    .col(NearNfts::OwnerId)
                    .col(NearNfts::ContractId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(NearNfts::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearTokenTransfers::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearTokens::Table).to_owned())
            .await?;
        Ok(())
    }
}

#[derive(DeriveIden)]
enum NearTokens {
    Table,
    ContractId,
    TokenStandard,
    Name,
    Symbol,
    Decimals,
    TotalSupply,
    Icon,
    Reference,
    ReferenceHash,
    BaseUri,
    FirstSeenHeight,
    FirstSeenTime,
    LastSeenHeight,
    LastSeenTime,
}

#[derive(DeriveIden)]
enum NearTokenTransfers {
    Table,
    Id,
    ContractId,
    TokenStandard,
    EventType,
    FromAccountId,
    ToAccountId,
    Amount,
    TokenIds,
    Memo,
    AuthorizedId,
    BlockHeight,
    BlockTime,
    TxHash,
    ReceiptHash,
    LogIndex,
}

#[derive(DeriveIden)]
enum NearNfts {
    Table,
    Id,
    ContractId,
    TokenId,
    OwnerId,
    ApprovedAccountIds,
    MetadataTitle,
    MetadataDescription,
    MetadataMedia,
    MetadataMediaHash,
    MetadataCopies,
    MetadataIssuedAt,
    MetadataExpiresAt,
    MetadataStartsAt,
    MetadataUpdatedAt,
    MetadataExtra,
    MetadataReference,
    MetadataReferenceHash,
    MintedHeight,
    MintedTime,
    MintedByTxHash,
    MintedByReceiptHash,
    LastTransferHeight,
    LastTransferTime,
    Burned,
    BurnedHeight,
    BurnedTime,
}
