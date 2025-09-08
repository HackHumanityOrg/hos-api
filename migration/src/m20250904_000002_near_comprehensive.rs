use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // =================
        // CORE TABLES
        // =================

        // near_transactions - All NEAR blockchain transactions
        manager
            .create_table(
                Table::create()
                    .table(NearTransactions::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearTransactions::TxHash)
                            .string_len(100)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearTransactions::SignerId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTransactions::ReceiverId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTransactions::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTransactions::BlockTime)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTransactions::BlockHash)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearTransactions::Actions).text()) // JSON array of actions
                    .col(
                        ColumnDef::new(NearTransactions::Status)
                            .string_len(32)
                            .not_null(),
                    ) // "success" or "failure"
                    .col(ColumnDef::new(NearTransactions::ReceiptIds).text()) // JSON array of receipt IDs
                    .col(ColumnDef::new(NearTransactions::PublicKey).string_len(128))
                    .col(ColumnDef::new(NearTransactions::Nonce).big_integer())
                    .col(ColumnDef::new(NearTransactions::GasUsed).big_integer())
                    .col(ColumnDef::new(NearTransactions::TokensBurnt).decimal_len(39, 0))
                    .col(ColumnDef::new(NearTransactions::ExecutorId).string_len(128)) // Account that executed the transaction
                    .col(ColumnDef::new(NearTransactions::PriorityFee).decimal_len(39, 0)) // Priority fee for transaction prioritization
                    .col(ColumnDef::new(NearTransactions::Signature).string_len(200)) // Cryptographic signature
                    .col(ColumnDef::new(NearTransactions::ExecutionOutcomeId).string_len(100)) // Execution outcome ID
                    .col(ColumnDef::new(NearTransactions::Logs).text()) // JSON array of execution logs
                    .col(ColumnDef::new(NearTransactions::Proof).text()) // JSON array of Merkle proof data
                    .col(ColumnDef::new(NearTransactions::Metadata).text()) // JSON execution metadata
                    .to_owned(),
            )
            .await?;

        // near_accounts
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
                    .col(ColumnDef::new(NearAccounts::ParentAccountId).string_len(128))
                    .col(ColumnDef::new(NearAccounts::Depth).integer().not_null())
                    .col(
                        ColumnDef::new(NearAccounts::IsTopLevel)
                            .boolean()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccounts::IsImplicit)
                            .boolean()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearAccounts::IsSystem).boolean().not_null())
                    .col(
                        ColumnDef::new(NearAccounts::IsStakingPool)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(NearAccounts::IsLockupContract)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(NearAccounts::StoragePaidAt)
                            .big_integer()
                            .null(),
                    )
                    .col(ColumnDef::new(NearAccounts::GlobalContractAccountId).string_len(128))
                    .col(ColumnDef::new(NearAccounts::GlobalContractHash).string_len(100))
                    .col(ColumnDef::new(NearAccounts::CreatedAtHeight).big_integer())
                    .col(ColumnDef::new(NearAccounts::CreatedAtTime).big_integer())
                    .col(ColumnDef::new(NearAccounts::CreatedCause).string_len(64))
                    .col(ColumnDef::new(NearAccounts::CreatedByTxHash).string_len(100))
                    .col(ColumnDef::new(NearAccounts::CreatedByReceiptHash).string_len(100))
                    .col(ColumnDef::new(NearAccounts::DeletedAtHeight).big_integer())
                    .col(ColumnDef::new(NearAccounts::DeletedAtTime).big_integer())
                    .col(ColumnDef::new(NearAccounts::DeletedBeneficiaryId).string_len(128))
                    .col(ColumnDef::new(NearAccounts::DeletedByTxHash).string_len(100))
                    .col(ColumnDef::new(NearAccounts::DeletedByReceiptHash).string_len(100))
                    .col(ColumnDef::new(NearAccounts::FirstSeenHeight).big_integer())
                    .col(ColumnDef::new(NearAccounts::FirstSeenTime).big_integer())
                    .col(ColumnDef::new(NearAccounts::LastSeenHeight).big_integer())
                    .col(ColumnDef::new(NearAccounts::LastSeenTime).big_integer())
                    .to_owned(),
            )
            .await?;

        // near_account_state_history
        manager
            .create_table(
                Table::create()
                    .table(NearAccountStateHistory::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearAccountStateHistory::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearAccountStateHistory::AccountId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountStateHistory::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountStateHistory::BlockTime)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountStateHistory::Amount)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountStateHistory::Locked)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountStateHistory::StorageUsage)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountStateHistory::CodeHash)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountStateHistory::GlobalContractAccountId)
                            .string_len(128),
                    )
                    .col(
                        ColumnDef::new(NearAccountStateHistory::GlobalContractHash).string_len(100),
                    )
                    .col(
                        ColumnDef::new(NearAccountStateHistory::CauseType)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearAccountStateHistory::CauseTxHash).string_len(100))
                    .col(ColumnDef::new(NearAccountStateHistory::CauseReceiptHash).string_len(100))
                    .to_owned(),
            )
            .await?;

        // near_access_keys - Access key state and changes
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
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::PublicKey)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::AccessKeyType)
                            .string_len(32)
                            .not_null(), // "full_access" or "function_call"
                    )
                    .col(ColumnDef::new(NearAccessKeys::ContractId).string_len(128)) // For function call keys
                    .col(ColumnDef::new(NearAccessKeys::MethodNames).text()) // JSON array for function call keys
                    .col(ColumnDef::new(NearAccessKeys::Allowance).decimal_len(39, 0)) // For function call keys
                    .col(ColumnDef::new(NearAccessKeys::Nonce).big_integer()) // Access key nonce
                    .col(
                        ColumnDef::new(NearAccessKeys::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::BlockTime)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::ChangeType)
                            .string_len(16)
                            .not_null(), // "add" or "delete"
                    )
                    .col(
                        ColumnDef::new(NearAccessKeys::CauseType)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearAccessKeys::CauseTxHash).string_len(100))
                    .col(ColumnDef::new(NearAccessKeys::CauseReceiptHash).string_len(100))
                    .col(ColumnDef::new(NearAccessKeys::DeletedAtHeight).big_integer())
                    .col(ColumnDef::new(NearAccessKeys::DeletedAtTime).big_integer())
                    .to_owned(),
            )
            .await?;

        // near_contract_code - Contract code deployments and updates
        manager
            .create_table(
                Table::create()
                    .table(NearContractCode::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearContractCode::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearContractCode::AccountId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearContractCode::CodeHash)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearContractCode::Code).blob()) // Contract binary code (4MB NEAR limit)
                    .col(
                        ColumnDef::new(NearContractCode::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearContractCode::BlockTime)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearContractCode::ChangeType)
                            .string_len(16)
                            .not_null(), // "deploy" or "delete"
                    )
                    .col(
                        ColumnDef::new(NearContractCode::CauseType)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearContractCode::CauseTxHash).string_len(100))
                    .col(ColumnDef::new(NearContractCode::CauseReceiptHash).string_len(100))
                    .to_owned(),
            )
            .await?;

        // near_contract_data - Contract storage key-value data
        manager
            .create_table(
                Table::create()
                    .table(NearContractData::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearContractData::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearContractData::AccountId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearContractData::DataKey)
                            .string_len(512) // Hex encoded key
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearContractData::DataValue).text()) // Hex encoded value, NULL for deletion
                    .col(
                        ColumnDef::new(NearContractData::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearContractData::BlockTime)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearContractData::ChangeType)
                            .string_len(16)
                            .not_null(), // "update" or "delete"
                    )
                    .col(
                        ColumnDef::new(NearContractData::CauseType)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearContractData::CauseTxHash).string_len(100))
                    .col(ColumnDef::new(NearContractData::CauseReceiptHash).string_len(100))
                    .to_owned(),
            )
            .await?;

        // near_account_events
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
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountEvents::EventType)
                            .string_len(32)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountEvents::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearAccountEvents::BlockTime)
                            .big_integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearAccountEvents::TxHash).string_len(100))
                    .col(ColumnDef::new(NearAccountEvents::ReceiptHash).string_len(100))
                    .col(ColumnDef::new(NearAccountEvents::Metadata).text())
                    .to_owned(),
            )
            .await?;

        // near_tokens - minimal registry for NEP-141 and NEP-171 contracts we observe
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
                    // metadata columns removed for now (we don't populate them yet)
                    .col(ColumnDef::new(NearTokens::FirstSeenHeight).big_integer())
                    .col(ColumnDef::new(NearTokens::FirstSeenTime).big_integer())
                    .col(ColumnDef::new(NearTokens::LastSeenHeight).big_integer())
                    .col(ColumnDef::new(NearTokens::LastSeenTime).big_integer())
                    .to_owned(),
            )
            .await?;

        // near_token_events - All token events (NEP-141/171/245)
        manager
            .create_table(
                Table::create()
                    .table(NearTokenEvents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearTokenEvents::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::ContractId)
                            .string_len(128)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::TokenStandard)
                            .string_len(16)
                            .not_null(), // "nep141" or "nep171"
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::EventType)
                            .string_len(32)
                            .not_null(), // "ft_mint", "ft_transfer", "ft_burn", "nft_mint", "nft_transfer", "nft_burn"
                    )
                    .col(ColumnDef::new(NearTokenEvents::FromAccountId).string_len(128)) // NULL for mint
                    .col(ColumnDef::new(NearTokenEvents::ToAccountId).string_len(128)) // NULL for burn
                    .col(ColumnDef::new(NearTokenEvents::Amount).decimal_len(39, 0)) // For NEP-141 transfers
                    .col(ColumnDef::new(NearTokenEvents::Amounts).text()) // JSON array for NEP-245 batch amounts
                    .col(ColumnDef::new(NearTokenEvents::TokenIds).text()) // JSON array for NEP-171/245
                    .col(ColumnDef::new(NearTokenEvents::Memo).text())
                    .col(ColumnDef::new(NearTokenEvents::AuthorizedId).string_len(128)) // For NFT burns
                    .col(
                        ColumnDef::new(NearTokenEvents::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::BlockTime)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::TxHash)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::ReceiptHash)
                            .string_len(100)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearTokenEvents::LogIndex)
                            .integer()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

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
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearValidatorProposals::ShardId).integer())
                    .to_owned(),
            )
            .await?;

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

        // NETWORK TABLES
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
                        ColumnDef::new(NearNetworkState::NextEpochId)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::BlockHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::PrevHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::ChunksIncluded)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::RentPaid)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::ValidatorReward)
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::LastFinalBlock)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::LastDsFinalBlock)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::NextBpHash)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::BlockMerkleRoot)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::RandomValue)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::Timestamp)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::TimestampNanosec)
                            .big_unsigned()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::BlockOrdinal)
                            .big_unsigned()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(NearNetworkState::PrevHeight)
                            .big_unsigned()
                            .null(),
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
                    .to_owned(),
            )
            .await?;

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
                        ColumnDef::new(NearValidatorRewards::BlockHeight)
                            .big_integer()
                            .not_null(),
                    ) // Add block-level granularity
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
                            .decimal_len(39, 0)
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorRewards::Timestamp)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearValidatorRewards::Source)
                            .string_len(16)
                            .not_null(), // "block" or "chunk"
                    )
                    .col(ColumnDef::new(NearValidatorRewards::ShardId).integer()) // For chunk rewards
                    .to_owned(),
            )
            .await?;

        // near_sync_cursors
        manager
            .create_table(
                Table::create()
                    .table(NearSyncCursors::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(NearSyncCursors::StreamName)
                            .string_len(64)
                            .not_null()
                            .primary_key(),
                    )
                    .col(
                        ColumnDef::new(NearSyncCursors::BlockHeight)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(NearSyncCursors::UpdatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(NearSyncCursors::Table).to_owned())
            .await?;
        // Drop consolidated validator/network tables
        manager
            .drop_table(Table::drop().table(NearValidatorRewards::Table).to_owned())
            .await?;
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
        // near_nfts drop removed (table not created)
        manager
            .drop_table(Table::drop().table(NearTokenEvents::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearTokens::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearAccountEvents::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearContractData::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearContractCode::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearAccessKeys::Table).to_owned())
            .await?;
        manager
            .drop_table(
                Table::drop()
                    .table(NearAccountStateHistory::Table)
                    .to_owned(),
            )
            .await?;
        manager
            .drop_table(Table::drop().table(NearAccounts::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearTransactions::Table).to_owned())
            .await?;
        Ok(())
    }
}

// =================
// TABLE IDENTIFIERS
// =================

#[derive(DeriveIden)]
enum NearTransactions {
    Table,
    TxHash,
    SignerId,
    ReceiverId,
    BlockHeight,
    BlockTime,
    BlockHash,
    Actions,
    Status,
    ReceiptIds,
    PublicKey,
    Nonce,
    GasUsed,
    TokensBurnt,
    ExecutorId,
    PriorityFee,
    Signature,
    ExecutionOutcomeId,
    Logs,
    Proof,
    Metadata,
}

#[derive(DeriveIden)]
enum NearAccounts {
    Table,
    AccountId,
    ParentAccountId,
    Depth,
    IsTopLevel,
    IsImplicit,
    IsSystem,
    IsStakingPool,
    IsLockupContract,
    StoragePaidAt,
    GlobalContractAccountId,
    GlobalContractHash,
    CreatedAtHeight,
    CreatedAtTime,
    CreatedCause,
    CreatedByTxHash,
    CreatedByReceiptHash,
    DeletedAtHeight,
    DeletedAtTime,
    DeletedBeneficiaryId,
    DeletedByTxHash,
    DeletedByReceiptHash,
    FirstSeenHeight,
    FirstSeenTime,
    LastSeenHeight,
    LastSeenTime,
}

#[derive(DeriveIden)]
enum NearAccountStateHistory {
    Table,
    Id,
    AccountId,
    BlockHeight,
    BlockTime,
    Amount,
    Locked,
    StorageUsage,
    CodeHash,
    GlobalContractAccountId,
    GlobalContractHash,
    CauseType,
    CauseTxHash,
    CauseReceiptHash,
}

#[derive(DeriveIden)]
enum NearAccessKeys {
    Table,
    Id,
    AccountId,
    PublicKey,
    AccessKeyType,
    ContractId,
    MethodNames,
    Allowance,
    Nonce,
    BlockHeight,
    BlockTime,
    ChangeType,
    CauseType,
    CauseTxHash,
    CauseReceiptHash,
    DeletedAtHeight,
    DeletedAtTime,
}

#[derive(DeriveIden)]
enum NearContractCode {
    Table,
    Id,
    AccountId,
    CodeHash,
    Code,
    BlockHeight,
    BlockTime,
    ChangeType,
    CauseType,
    CauseTxHash,
    CauseReceiptHash,
}

#[derive(DeriveIden)]
enum NearContractData {
    Table,
    Id,
    AccountId,
    DataKey,
    DataValue,
    BlockHeight,
    BlockTime,
    ChangeType,
    CauseType,
    CauseTxHash,
    CauseReceiptHash,
}

#[derive(DeriveIden)]
enum NearAccountEvents {
    Table,
    Id,
    AccountId,
    EventType,
    BlockHeight,
    BlockTime,
    TxHash,
    ReceiptHash,
    Metadata,
}

#[derive(DeriveIden)]
enum NearTokens {
    Table,
    ContractId,
    TokenStandard,
    FirstSeenHeight,
    FirstSeenTime,
    LastSeenHeight,
    LastSeenTime,
}

#[derive(DeriveIden)]
enum NearTokenEvents {
    Table,
    Id,
    ContractId,
    TokenStandard,
    EventType,
    FromAccountId,
    ToAccountId,
    Amount,
    Amounts,
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
enum NearSyncCursors {
    Table,
    StreamName,
    BlockHeight,
    UpdatedAt,
}

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
    NextEpochId,
    BlockHash,
    PrevHash,
    ChunksIncluded,
    RentPaid,
    ValidatorReward,
    LastFinalBlock,
    LastDsFinalBlock,
    NextBpHash,
    BlockMerkleRoot,
    RandomValue,
    Timestamp,
    TimestampNanosec,
    BlockOrdinal,
    PrevHeight,
    GasPrice,
    ProtocolVersion,
    TotalSupply,
}

#[derive(DeriveIden)]
enum NearValidatorRewards {
    Table,
    Id,
    ValidatorId,
    EpochId,
    BlockHeight,
    RewardAmount,
    DelegatorRewards,
    CommissionEarned,
    CommissionRate,
    Timestamp,
    Source,
    ShardId,
}
