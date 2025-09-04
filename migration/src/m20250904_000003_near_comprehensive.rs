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
                    .col(ColumnDef::new(NearTransactions::GasUsed).big_integer())
                    .col(ColumnDef::new(NearTransactions::TokensBurnt).decimal_len(39, 0))
                    .to_owned(),
            )
            .await?;

        // Indexes for near_transactions
        manager
            .create_index(
                Index::create()
                    .name("idx_near_transactions_signer")
                    .table(NearTransactions::Table)
                    .col(NearTransactions::SignerId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_transactions_receiver")
                    .table(NearTransactions::Table)
                    .col(NearTransactions::ReceiverId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_transactions_block_height")
                    .table(NearTransactions::Table)
                    .col(NearTransactions::BlockHeight)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_transactions_block_time")
                    .table(NearTransactions::Table)
                    .col(NearTransactions::BlockTime)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_transactions_status")
                    .table(NearTransactions::Table)
                    .col(NearTransactions::Status)
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

        // FK: near_accounts.created_by_tx_hash -> near_transactions(tx_hash)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_accounts_created_tx")
                    .from(NearAccounts::Table, NearAccounts::CreatedByTxHash)
                    .to(NearTransactions::Table, NearTransactions::TxHash)
                    .on_delete(ForeignKeyAction::SetNull)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // FK: near_accounts.deleted_by_tx_hash -> near_transactions(tx_hash)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_accounts_deleted_tx")
                    .from(NearAccounts::Table, NearAccounts::DeletedByTxHash)
                    .to(NearTransactions::Table, NearTransactions::TxHash)
                    .on_delete(ForeignKeyAction::SetNull)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // Performance indexes for near_accounts
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_deleted_beneficiary")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::DeletedBeneficiaryId)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_created_height")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::CreatedAtHeight)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_deleted_height")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::DeletedAtHeight)
                    .to_owned(),
            )
            .await?;

        // Account type filtering indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_is_implicit")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::IsImplicit)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_is_system")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::IsSystem)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_is_top_level")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::IsTopLevel)
                    .to_owned(),
            )
            .await?;

        // Transaction and receipt hash indexes for tracking
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_created_by_tx")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::CreatedByTxHash)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_deleted_by_tx")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::DeletedByTxHash)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_created_by_receipt")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::CreatedByReceiptHash)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_deleted_by_receipt")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::DeletedByReceiptHash)
                    .to_owned(),
            )
            .await?;

        // Time-based indexes for temporal queries
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_first_seen_time")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::FirstSeenTime)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_last_seen_time")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::LastSeenTime)
                    .to_owned(),
            )
            .await?;

        // Composite indexes for common query patterns
        manager
            .create_index(
                Index::create()
                    .name("idx_near_accounts_parent_depth")
                    .table(NearAccounts::Table)
                    .col(NearAccounts::ParentAccountId)
                    .col(NearAccounts::Depth)
                    .to_owned(),
            )
            .await?;

        // =================
        // STATE CHANGE TABLES
        // =================

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
                        ColumnDef::new(NearAccountStateHistory::CauseType)
                            .string_len(64)
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearAccountStateHistory::CauseTxHash).string_len(100))
                    .col(ColumnDef::new(NearAccountStateHistory::CauseReceiptHash).string_len(100))
                    .to_owned(),
            )
            .await?;

        // FK: near_account_state_history.account_id -> near_accounts(account_id)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_account_state_account")
                    .from(
                        NearAccountStateHistory::Table,
                        NearAccountStateHistory::AccountId,
                    )
                    .to(NearAccounts::Table, NearAccounts::AccountId)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // FK: near_account_state_history.cause_tx_hash -> near_transactions(tx_hash)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_account_state_cause_tx")
                    .from(
                        NearAccountStateHistory::Table,
                        NearAccountStateHistory::CauseTxHash,
                    )
                    .to(NearTransactions::Table, NearTransactions::TxHash)
                    .on_delete(ForeignKeyAction::SetNull)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ux_near_account_state_account_height")
                    .table(NearAccountStateHistory::Table)
                    .col(NearAccountStateHistory::AccountId)
                    .col(NearAccountStateHistory::BlockHeight)
                    .unique()
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_near_account_state_height")
                    .table(NearAccountStateHistory::Table)
                    .col(NearAccountStateHistory::BlockHeight)
                    .to_owned(),
            )
            .await?;
        // Time-based index for state history
        manager
            .create_index(
                Index::create()
                    .name("idx_near_account_state_block_time")
                    .table(NearAccountStateHistory::Table)
                    .col(NearAccountStateHistory::BlockTime)
                    .to_owned(),
            )
            .await?;
        // Cause tracking indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_near_account_state_cause_type")
                    .table(NearAccountStateHistory::Table)
                    .col(NearAccountStateHistory::CauseType)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_near_account_state_cause_tx")
                    .table(NearAccountStateHistory::Table)
                    .col(NearAccountStateHistory::CauseTxHash)
                    .to_owned(),
            )
            .await?;
        manager
            .create_index(
                Index::create()
                    .name("idx_near_account_state_cause_receipt")
                    .table(NearAccountStateHistory::Table)
                    .col(NearAccountStateHistory::CauseReceiptHash)
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
                    .col(
                        ColumnDef::new(NearAccessKeys::IsDeleted)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .col(ColumnDef::new(NearAccessKeys::DeletedAtHeight).big_integer())
                    .col(ColumnDef::new(NearAccessKeys::DeletedAtTime).big_integer())
                    .to_owned(),
            )
            .await?;

        // FK: near_access_keys.account_id -> near_accounts(account_id)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_access_keys_account")
                    .from(NearAccessKeys::Table, NearAccessKeys::AccountId)
                    .to(NearAccounts::Table, NearAccounts::AccountId)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // FK: near_access_keys.cause_tx_hash -> near_transactions(tx_hash)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_access_keys_cause_tx")
                    .from(NearAccessKeys::Table, NearAccessKeys::CauseTxHash)
                    .to(NearTransactions::Table, NearTransactions::TxHash)
                    .on_delete(ForeignKeyAction::SetNull)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // Indexes for near_access_keys
        manager
            .create_index(
                Index::create()
                    .name("ux_near_access_keys_account_key_height")
                    .table(NearAccessKeys::Table)
                    .col(NearAccessKeys::AccountId)
                    .col(NearAccessKeys::PublicKey)
                    .col(NearAccessKeys::BlockHeight)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_access_keys_public_key")
                    .table(NearAccessKeys::Table)
                    .col(NearAccessKeys::PublicKey)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_access_keys_type")
                    .table(NearAccessKeys::Table)
                    .col(NearAccessKeys::AccessKeyType)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_access_keys_change_type")
                    .table(NearAccessKeys::Table)
                    .col(NearAccessKeys::ChangeType)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_access_keys_block_time")
                    .table(NearAccessKeys::Table)
                    .col(NearAccessKeys::BlockTime)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_access_keys_is_deleted")
                    .table(NearAccessKeys::Table)
                    .col(NearAccessKeys::IsDeleted)
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
                    .col(ColumnDef::new(NearContractCode::Code).binary()) // Contract binary code
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

        // FK: near_contract_code.account_id -> near_accounts(account_id)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_contract_code_account")
                    .from(NearContractCode::Table, NearContractCode::AccountId)
                    .to(NearAccounts::Table, NearAccounts::AccountId)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // FK: near_contract_code.cause_tx_hash -> near_transactions(tx_hash)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_contract_code_cause_tx")
                    .from(NearContractCode::Table, NearContractCode::CauseTxHash)
                    .to(NearTransactions::Table, NearTransactions::TxHash)
                    .on_delete(ForeignKeyAction::SetNull)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // Indexes for near_contract_code
        manager
            .create_index(
                Index::create()
                    .name("ux_near_contract_code_account_height")
                    .table(NearContractCode::Table)
                    .col(NearContractCode::AccountId)
                    .col(NearContractCode::BlockHeight)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_contract_code_hash")
                    .table(NearContractCode::Table)
                    .col(NearContractCode::CodeHash)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_contract_code_block_time")
                    .table(NearContractCode::Table)
                    .col(NearContractCode::BlockTime)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_contract_code_change_type")
                    .table(NearContractCode::Table)
                    .col(NearContractCode::ChangeType)
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
                            .string_len(512) // Base64 encoded key
                            .not_null(),
                    )
                    .col(ColumnDef::new(NearContractData::DataValue).text()) // Base64 encoded value, NULL for deletion
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

        // FK: near_contract_data.account_id -> near_accounts(account_id)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_contract_data_account")
                    .from(NearContractData::Table, NearContractData::AccountId)
                    .to(NearAccounts::Table, NearAccounts::AccountId)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // FK: near_contract_data.cause_tx_hash -> near_transactions(tx_hash)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_contract_data_cause_tx")
                    .from(NearContractData::Table, NearContractData::CauseTxHash)
                    .to(NearTransactions::Table, NearTransactions::TxHash)
                    .on_delete(ForeignKeyAction::SetNull)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // Indexes for near_contract_data
        manager
            .create_index(
                Index::create()
                    .name("ux_near_contract_data_account_key_height")
                    .table(NearContractData::Table)
                    .col(NearContractData::AccountId)
                    .col(NearContractData::DataKey)
                    .col(NearContractData::BlockHeight)
                    .unique()
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_contract_data_block_time")
                    .table(NearContractData::Table)
                    .col(NearContractData::BlockTime)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_contract_data_change_type")
                    .table(NearContractData::Table)
                    .col(NearContractData::ChangeType)
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

        // FK: near_account_events.account_id -> near_accounts(account_id)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_account_events_account")
                    .from(NearAccountEvents::Table, NearAccountEvents::AccountId)
                    .to(NearAccounts::Table, NearAccounts::AccountId)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // FK: near_account_events.tx_hash -> near_transactions(tx_hash)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_account_events_tx")
                    .from(NearAccountEvents::Table, NearAccountEvents::TxHash)
                    .to(NearTransactions::Table, NearTransactions::TxHash)
                    .on_delete(ForeignKeyAction::SetNull)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("ux_near_account_events_dedupe")
                    .table(NearAccountEvents::Table)
                    .col(NearAccountEvents::AccountId)
                    .col(NearAccountEvents::EventType)
                    .col(NearAccountEvents::BlockHeight)
                    .col(NearAccountEvents::ReceiptHash)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // Time-based index for events
        manager
            .create_index(
                Index::create()
                    .name("idx_near_account_events_block_time")
                    .table(NearAccountEvents::Table)
                    .col(NearAccountEvents::BlockTime)
                    .to_owned(),
            )
            .await?;

        // Transaction and receipt tracking for events
        manager
            .create_index(
                Index::create()
                    .name("idx_near_account_events_tx_hash")
                    .table(NearAccountEvents::Table)
                    .col(NearAccountEvents::TxHash)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_near_account_events_receipt_hash")
                    .table(NearAccountEvents::Table)
                    .col(NearAccountEvents::ReceiptHash)
                    .to_owned(),
            )
            .await?;

        // Composite index for event type and time queries
        manager
            .create_index(
                Index::create()
                    .name("idx_near_account_events_type_time")
                    .table(NearAccountEvents::Table)
                    .col(NearAccountEvents::EventType)
                    .col(NearAccountEvents::BlockTime)
                    .to_owned(),
            )
            .await?;

        // =================
        // TOKEN TABLES
        // =================

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

        // FK: near_tokens.contract_id -> near_accounts(account_id)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_tokens_account")
                    .from(NearTokens::Table, NearTokens::ContractId)
                    .to(NearAccounts::Table, NearAccounts::AccountId)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
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

        // FK: near_token_transfers.tx_hash -> near_transactions(tx_hash)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_token_transfers_tx")
                    .from(NearTokenTransfers::Table, NearTokenTransfers::TxHash)
                    .to(NearTransactions::Table, NearTransactions::TxHash)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // FK: near_token_transfers.from_account_id -> near_accounts(account_id)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_token_transfers_from")
                    .from(NearTokenTransfers::Table, NearTokenTransfers::FromAccountId)
                    .to(NearAccounts::Table, NearAccounts::AccountId)
                    .on_delete(ForeignKeyAction::SetNull)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // FK: near_token_transfers.to_account_id -> near_accounts(account_id)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_token_transfers_to")
                    .from(NearTokenTransfers::Table, NearTokenTransfers::ToAccountId)
                    .to(NearAccounts::Table, NearAccounts::AccountId)
                    .on_delete(ForeignKeyAction::SetNull)
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

        // FK: near_nfts.owner_id -> near_accounts(account_id)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_nfts_owner")
                    .from(NearNfts::Table, NearNfts::OwnerId)
                    .to(NearAccounts::Table, NearAccounts::AccountId)
                    .on_delete(ForeignKeyAction::SetNull)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        // FK: near_nfts.minted_by_tx_hash -> near_transactions(tx_hash)
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_near_nfts_minted_tx")
                    .from(NearNfts::Table, NearNfts::MintedByTxHash)
                    .to(NearTransactions::Table, NearTransactions::TxHash)
                    .on_delete(ForeignKeyAction::SetNull)
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

        // =================
        // UTILITY TABLES
        // =================

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
        manager
            .drop_table(Table::drop().table(NearNfts::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(NearTokenTransfers::Table).to_owned())
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
    GasUsed,
    TokensBurnt,
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
    BlockHeight,
    BlockTime,
    ChangeType,
    CauseType,
    CauseTxHash,
    CauseReceiptHash,
    IsDeleted,
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

#[derive(DeriveIden)]
enum NearSyncCursors {
    Table,
    StreamName,
    BlockHeight,
    UpdatedAt,
}
