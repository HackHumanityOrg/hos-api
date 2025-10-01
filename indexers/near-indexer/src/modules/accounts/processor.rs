use anyhow::{Context, Result};
use async_trait::async_trait;
use entities::near::*;
use near_lake_framework::near_indexer_primitives::{
    views::{AccessKeyPermissionView, StateChangeCauseView, StateChangeValueView},
    CryptoHash, IndexerShard, StreamerMessage,
};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    Set, TransactionTrait,
};
use serde_json;
use std::collections::{HashMap, HashSet};
use std::time::Instant;
use tracing::{debug, info, instrument, trace};

/// Type alias for account event tuples to reduce complexity
type AccountEvent = (
    String,            // account_id
    String,            // event_type
    Option<String>,    // related_account_id
    Option<String>,    // transaction_hash
    serde_json::Value, // event_data
);

/// Struct to group access key parameters
struct AccessKeyData {
    account_id: String,
    public_key: String,
    permission_type: String,
    allowance: Option<String>,
    receiver_id: Option<String>,
    method_names: Option<serde_json::Value>,
    nonce: i64,
    block_height: i64,
}

use super::classification::{classify_account, get_parent_account, validate_account_id};
use crate::core::cursor;
use crate::IndexerModule;

const MODULE_NAME: &str = "accounts";

pub struct AccountsModule {
    enabled: bool,
}

impl AccountsModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        let enabled = match config {
            common::config::AppConfig::Indexer(idx) => {
                let idx = idx.as_ref();
                let near_config = idx.near.as_ref();

                // Check if NEAR indexing is enabled at all
                let near_enabled = near_config.map(|c| c.chain != "disabled").unwrap_or(false);

                if !near_enabled {
                    return Self { enabled: false };
                }

                // Get accounts module configuration
                near_config
                    .and_then(|c| c.modules.as_ref())
                    .and_then(|m| m.accounts.as_ref())
                    .map(|a| a.enabled)
                    .unwrap_or(true) // Default enabled if not specified
            }
            _ => false,
        };

        Self { enabled }
    }

    /// Validate public key format
    fn validate_public_key(&self, public_key: &str) -> Result<()> {
        if public_key.is_empty() {
            return Err(anyhow::anyhow!("Public key cannot be empty"));
        }

        // Basic length check - NEAR public keys are typically base58 encoded
        // Ed25519 keys are usually around 43-44 characters when base58 encoded
        // Secp256k1 keys can be longer
        if public_key.len() < 20 || public_key.len() > 128 {
            return Err(anyhow::anyhow!(
                "Public key has invalid length: {}",
                public_key.len()
            ));
        }

        // Check that it contains valid base58 characters
        for char in public_key.chars() {
            if !char.is_ascii_alphanumeric() {
                // Base58 excludes 0, O, I, l to avoid confusion
                if "0OIl".contains(char) {
                    return Err(anyhow::anyhow!(
                        "Public key contains invalid base58 character: {}",
                        char
                    ));
                }
            }
        }

        Ok(())
    }

    /// Extract transaction/receipt hash from state change cause
    fn extract_transaction_context(
        &self,
        cause: &StateChangeCauseView,
    ) -> (Option<String>, Option<String>) {
        match cause {
            StateChangeCauseView::TransactionProcessing { tx_hash } => {
                (Some(tx_hash.to_string()), None)
            }
            StateChangeCauseView::ActionReceiptProcessingStarted { receipt_hash } => {
                (None, Some(receipt_hash.to_string()))
            }
            StateChangeCauseView::ActionReceiptGasReward { receipt_hash } => {
                (None, Some(receipt_hash.to_string()))
            }
            StateChangeCauseView::ReceiptProcessing { receipt_hash } => {
                (None, Some(receipt_hash.to_string()))
            }
            StateChangeCauseView::PostponedReceipt { receipt_hash } => {
                (None, Some(receipt_hash.to_string()))
            }
            _ => (None, None),
        }
    }

    /// Process state changes to extract account-related data
    async fn process_state_changes(
        &self,
        shards: &[IndexerShard],
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
        known_accounts: &mut HashSet<String>,
    ) -> Result<()> {
        let mut accounts_to_create = HashMap::new();
        let mut account_balances = Vec::new();
        let mut account_events = Vec::new();

        for shard in shards {
            // Use state changes directly - they're already parsed
            for state_change in &shard.state_changes {
                let (tx_hash, receipt_hash) = self.extract_transaction_context(&state_change.cause);

                match &state_change.value {
                    StateChangeValueView::AccountUpdate {
                        account_id,
                        account,
                    } => {
                        let account_id_str = account_id.to_string();

                        // Validate account ID format
                        if let Err(e) = validate_account_id(&account_id_str) {
                            trace!(account_id = %account_id_str, error = %e, "Invalid account ID format, skipping");
                            continue;
                        }

                        // Check if this is a new account (first check cache, then database)
                        let is_new_account = if known_accounts.contains(&account_id_str) {
                            false
                        } else {
                            // Check database for existing account
                            let existing = near_accounts::Entity::find()
                                .filter(near_accounts::Column::AccountId.eq(&account_id_str))
                                .one(db)
                                .await
                                .context("Failed to check for existing account")?;
                            existing.is_none()
                        };

                        if is_new_account {
                            let account_type = classify_account(&account_id_str).into();
                            let parent_account = get_parent_account(&account_id_str);
                            let is_contract = account.code_hash != CryptoHash::default();

                            accounts_to_create.insert(
                                account_id_str.clone(),
                                (account_type, parent_account, is_contract),
                            );

                            // Add to known accounts cache
                            known_accounts.insert(account_id_str.clone());

                            // Record account creation event
                            account_events.push((
                                account_id_str.clone(),
                                "created".to_string(),
                                tx_hash.clone(),
                                receipt_hash.clone(),
                                serde_json::json!({"cause": state_change.cause}),
                            ));
                        } else {
                            // Add to cache even if it already exists in DB
                            known_accounts.insert(account_id_str.clone());
                        }

                        // Always record balance update
                        account_balances.push((
                            account_id_str.clone(),
                            account.amount.to_string(),
                            account.locked.to_string(),
                            account.storage_usage as i64,
                            account.code_hash.to_string(),
                        ));

                        // Record contract deployment event if code changed
                        let is_contract = account.code_hash != CryptoHash::default();
                        if is_contract {
                            account_events.push((
                                account_id_str.clone(),
                                "contract_deployed".to_string(),
                                tx_hash.clone(),
                                receipt_hash.clone(),
                                serde_json::json!({
                                    "code_hash": account.code_hash.to_string(),
                                    "cause": state_change.cause
                                }),
                            ));
                        }
                    }
                    StateChangeValueView::AccountDeletion { account_id } => {
                        let account_id_str = account_id.to_string();

                        // Update the account record to mark it as deleted
                        self.mark_account_deleted(
                            &account_id_str,
                            block_height,
                            block_timestamp,
                            db,
                        )
                        .await
                        .context("Failed to mark account as deleted")?;

                        // Record account deletion event
                        account_events.push((
                            account_id_str.clone(),
                            "deleted".to_string(),
                            tx_hash.clone(),
                            receipt_hash.clone(),
                            serde_json::json!({"cause": state_change.cause}),
                        ));

                        // Remove from known accounts cache
                        known_accounts.remove(&account_id_str);
                    }
                    StateChangeValueView::AccessKeyUpdate {
                        account_id,
                        public_key,
                        access_key,
                    } => {
                        let account_id_str = account_id.to_string();
                        let public_key_str = public_key.to_string();

                        // Validate account ID and public key format
                        if let Err(e) = validate_account_id(&account_id_str) {
                            trace!(account_id = %account_id_str, error = %e, "Invalid account ID format for access key, skipping");
                            continue;
                        }

                        if let Err(e) = self.validate_public_key(&public_key_str) {
                            trace!(account_id = %account_id_str, public_key = %public_key_str, error = %e, "Invalid public key format, skipping");
                            continue;
                        }
                        let (permission_type, allowance, receiver_id, method_names) =
                            match &access_key.permission {
                                AccessKeyPermissionView::FullAccess => {
                                    ("FullAccess".to_string(), None, None, None)
                                }
                                AccessKeyPermissionView::FunctionCall {
                                    allowance,
                                    receiver_id,
                                    method_names,
                                } => (
                                    "FunctionCall".to_string(),
                                    allowance.map(|a| a.to_string()),
                                    Some(receiver_id.clone()),
                                    serde_json::to_value(method_names).ok(),
                                ),
                            };

                        // Handle access key upsert directly in the database
                        self.upsert_access_key(
                            AccessKeyData {
                                account_id: account_id_str.clone(),
                                public_key: public_key_str.clone(),
                                permission_type,
                                allowance,
                                receiver_id,
                                method_names,
                                nonce: access_key.nonce as i64,
                                block_height,
                            },
                            db,
                        )
                        .await
                        .context("Failed to upsert access key")?;
                    }
                    StateChangeValueView::AccessKeyDeletion {
                        account_id,
                        public_key,
                    } => {
                        let account_id_str = account_id.to_string();
                        let public_key_str = public_key.to_string();

                        // Validate account ID and public key format
                        if let Err(e) = validate_account_id(&account_id_str) {
                            trace!(account_id = %account_id_str, error = %e, "Invalid account ID format for access key deletion, skipping");
                            continue;
                        }

                        if let Err(e) = self.validate_public_key(&public_key_str) {
                            trace!(account_id = %account_id_str, public_key = %public_key_str, error = %e, "Invalid public key format for deletion, skipping");
                            continue;
                        }

                        // Mark access key as deleted in the database
                        self.mark_access_key_deleted(
                            &account_id_str,
                            &public_key_str,
                            block_height,
                            db,
                        )
                        .await
                        .context("Failed to mark access key as deleted")?;
                    }
                    StateChangeValueView::ContractCodeUpdate { account_id, .. } => {
                        // Record contract update event
                        account_events.push((
                            account_id.to_string(),
                            "contract_updated".to_string(),
                            tx_hash.clone(),
                            receipt_hash.clone(),
                            serde_json::json!({"cause": state_change.cause}),
                        ));
                    }
                    StateChangeValueView::ContractCodeDeletion { account_id } => {
                        // Record contract deletion event
                        account_events.push((
                            account_id.to_string(),
                            "contract_deleted".to_string(),
                            tx_hash.clone(),
                            receipt_hash.clone(),
                            serde_json::json!({"cause": state_change.cause}),
                        ));
                    }
                    _ => {
                        // Other state changes (DataUpdate, DataDeletion)
                        // are not directly account-related for our purposes
                    }
                }
            }
        }

        // Batch insert all the collected data
        self.store_accounts(accounts_to_create, block_height, block_timestamp, db)
            .await?;
        self.store_account_balances(account_balances, block_height, block_timestamp, db)
            .await?;
        self.store_account_events(account_events, block_height, block_timestamp, db)
            .await?;

        Ok(())
    }

    /// Store new accounts
    async fn store_accounts(
        &self,
        accounts: HashMap<String, (String, Option<String>, bool)>,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        if accounts.is_empty() {
            return Ok(());
        }

        let mut account_records = Vec::new();

        for (account_id, (account_type, parent_account, is_contract)) in accounts {
            let account_record = near_accounts::ActiveModel {
                account_id: Set(account_id),
                account_type: Set(account_type),
                created_at_height: Set(block_height),
                created_at_timestamp: Set(block_timestamp),
                deleted_at_height: Set(None),
                deleted_at_timestamp: Set(None),
                parent_account_id: Set(parent_account),
                is_contract: Set(is_contract),
            };
            account_records.push(account_record);
        }

        if !account_records.is_empty() {
            near_accounts::Entity::insert_many(account_records)
                .on_conflict_do_nothing()
                .exec(db)
                .await
                .context("Failed to insert account records")?;
        }

        Ok(())
    }

    /// Store account balance updates
    async fn store_account_balances(
        &self,
        balances: Vec<(String, String, String, i64, String)>,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        if balances.is_empty() {
            return Ok(());
        }

        let mut balance_records = Vec::new();

        for (account_id, amount, locked, storage_usage, code_hash) in balances {
            let balance_record = near_account_balances::ActiveModel {
                id: sea_orm::ActiveValue::NotSet,
                account_id: Set(account_id),
                block_height: Set(block_height),
                timestamp: Set(block_timestamp),
                amount: Set(amount),
                locked: Set(locked),
                storage_usage: Set(storage_usage),
                code_hash: Set(code_hash),
            };
            balance_records.push(balance_record);
        }

        near_account_balances::Entity::insert_many(balance_records)
            .exec(db)
            .await
            .context("Failed to insert account balance records")?;

        Ok(())
    }

    /// Store account events
    async fn store_account_events(
        &self,
        events: Vec<AccountEvent>,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let mut event_records = Vec::new();

        for (account_id, event_type, transaction_hash, receipt_id, metadata) in events {
            let event_record = near_account_events::ActiveModel {
                id: sea_orm::ActiveValue::NotSet,
                account_id: Set(account_id),
                event_type: Set(event_type),
                block_height: Set(block_height),
                timestamp: Set(block_timestamp),
                transaction_hash: Set(transaction_hash),
                receipt_id: Set(receipt_id),
                metadata: Set(Some(metadata)),
            };
            event_records.push(event_record);
        }

        near_account_events::Entity::insert_many(event_records)
            .exec(db)
            .await
            .context("Failed to insert account event records")?;

        Ok(())
    }

    /// Mark an account as deleted in the database
    async fn mark_account_deleted(
        &self,
        account_id: &str,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // Find and update the account record
        if let Some(account) = near_accounts::Entity::find()
            .filter(near_accounts::Column::AccountId.eq(account_id))
            .one(db)
            .await?
        {
            let mut account: near_accounts::ActiveModel = account.into();
            account.deleted_at_height = Set(Some(block_height));
            account.deleted_at_timestamp = Set(Some(block_timestamp));

            account
                .update(db)
                .await
                .context("Failed to update account deletion")?;
            debug!(
                account_id = account_id,
                height = block_height,
                "Marked account as deleted"
            );
        } else {
            // Account not found in database - this could happen if we're processing
            // a deletion before the creation, which shouldn't happen in normal operation
            debug!(
                account_id = account_id,
                "Account not found when trying to mark as deleted"
            );
        }

        Ok(())
    }

    /// Upsert an access key (insert if new, update if exists)
    async fn upsert_access_key(
        &self,
        key_data: AccessKeyData,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // Check if access key already exists
        if let Some(existing_key) = near_access_keys::Entity::find()
            .filter(near_access_keys::Column::AccountId.eq(&key_data.account_id))
            .filter(near_access_keys::Column::PublicKey.eq(&key_data.public_key))
            .filter(near_access_keys::Column::IsActive.eq(1))
            .one(db)
            .await?
        {
            // Update existing key
            let mut existing_key: near_access_keys::ActiveModel = existing_key.into();
            existing_key.permission_type = Set(key_data.permission_type);
            existing_key.allowance = Set(key_data.allowance);
            existing_key.receiver_id = Set(key_data.receiver_id);
            existing_key.method_names = Set(key_data.method_names);
            existing_key.nonce = Set(key_data.nonce);

            existing_key
                .update(db)
                .await
                .context("Failed to update access key")?;
            debug!(
                account_id = %key_data.account_id,
                public_key = %key_data.public_key,
                "Updated existing access key"
            );
        } else {
            // Insert new key
            let new_key = near_access_keys::ActiveModel {
                id: sea_orm::ActiveValue::NotSet,
                account_id: Set(key_data.account_id.clone()),
                public_key: Set(key_data.public_key.clone()),
                permission_type: Set(key_data.permission_type),
                allowance: Set(key_data.allowance),
                receiver_id: Set(key_data.receiver_id),
                method_names: Set(key_data.method_names),
                nonce: Set(key_data.nonce),
                created_at_height: Set(key_data.block_height),
                deleted_at_height: Set(None),
                is_active: Set(true),
            };

            new_key
                .insert(db)
                .await
                .context("Failed to insert new access key")?;
            debug!(
                account_id = %key_data.account_id,
                public_key = %key_data.public_key,
                "Inserted new access key"
            );
        }

        Ok(())
    }

    /// Mark an access key as deleted in the database
    async fn mark_access_key_deleted(
        &self,
        account_id: &str,
        public_key: &str,
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // Find and update the access key record
        if let Some(access_key) = near_access_keys::Entity::find()
            .filter(near_access_keys::Column::AccountId.eq(account_id))
            .filter(near_access_keys::Column::PublicKey.eq(public_key))
            .filter(near_access_keys::Column::IsActive.eq(1))
            .one(db)
            .await?
        {
            let mut access_key: near_access_keys::ActiveModel = access_key.into();
            access_key.deleted_at_height = Set(Some(block_height));
            access_key.is_active = Set(false);

            access_key
                .update(db)
                .await
                .context("Failed to update access key deletion")?;
            debug!(
                account_id = account_id,
                public_key = public_key,
                height = block_height,
                "Marked access key as deleted"
            );
        } else {
            // Access key not found in database
            debug!(
                account_id = account_id,
                public_key = public_key,
                "Access key not found when trying to mark as deleted"
            );
        }

        Ok(())
    }

    /// Load existing accounts from database to populate cache
    async fn load_existing_accounts(&self, _db: &DatabaseConnection) -> Result<()> {
        // This is a simplified approach - in production you might want to be more selective
        // or use a more efficient caching strategy
        debug!("Loading existing accounts into cache...");

        // For now, we'll start with an empty cache and build it as we process
        // In a production system, you might want to load from the database
        // Note: cache will be passed as mutable parameter to process_state_changes

        debug!("Account cache initialized");
        Ok(())
    }
}

#[async_trait]
impl IndexerModule for AccountsModule {
    fn name(&self) -> &str {
        MODULE_NAME
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    async fn initialize(&self, db: &DatabaseConnection) -> Result<()> {
        self.load_existing_accounts(db).await?;
        info!("Accounts module initialized");
        Ok(())
    }

    #[instrument(skip(self, msg, db), fields(module = "accounts", height = %msg.block.header.height))]
    async fn process_block(&self, msg: &StreamerMessage, db: &DatabaseConnection) -> Result<()> {
        let block_height = msg.block.header.height as i64;
        let block_timestamp = msg.block.header.timestamp_nanosec as i64;
        let start_time = Instant::now();

        info!(height = block_height, "Processing accounts for block");

        let txn = db.begin().await?;

        // Use a local cache for this block processing
        let mut known_accounts = HashSet::new();

        self.process_state_changes(
            &msg.shards,
            block_height,
            block_timestamp,
            &txn,
            &mut known_accounts,
        )
        .await
        .context("Failed to process state changes for accounts")?;

        txn.commit().await?;

        let elapsed = start_time.elapsed();
        info!(
            height = block_height,
            accounts_processed = known_accounts.len(),
            elapsed_ms = elapsed.as_millis(),
            "Successfully processed accounts for block"
        );

        Ok(())
    }

    async fn get_cursor(&self, db: &DatabaseConnection) -> Result<Option<i64>> {
        cursor::get_module_cursor(db, MODULE_NAME).await
    }

    async fn update_cursor(&self, db: &DatabaseConnection, height: i64) -> Result<()> {
        cursor::upsert_module_cursor(db, MODULE_NAME, height).await
    }
}
