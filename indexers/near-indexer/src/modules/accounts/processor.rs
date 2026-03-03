use anyhow::{Context, Result};
use async_trait::async_trait;
use common::config::NearModuleStreamMode;
use entities::near::*;
use near_lake_framework::near_indexer_primitives::{
    types::ShardId,
    views::{AccessKeyPermissionView, StateChangeCauseView, StateChangeValueView},
    CryptoHash, IndexerShard, StreamerMessage,
};
use sea_orm::{
    sea_query::OnConflict, ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection,
    EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};
use serde_json;
use std::collections::HashMap;
use std::time::Instant;
use tracing::{debug, info, instrument, trace};

#[derive(Debug, Clone)]
struct AccountBalanceUpdate {
    account_id: String,
    amount: String,
    locked: String,
    storage_usage: i64,
    code_hash: String,
    shard_id: i32,
    state_change_index: i32,
}

#[derive(Debug, Clone)]
struct AccountEventRecord {
    account_id: String,
    event_type: String,
    transaction_hash: Option<String>,
    receipt_id: Option<String>,
    metadata: serde_json::Value,
    shard_id: i32,
    state_change_index: i32,
    event_index: i32,
}

#[derive(Debug, Clone)]
struct AccountSnapshotUpsert {
    account_id: String,
    is_contract: Option<bool>,
    clear_deleted: bool,
    global_contract_account_id: Option<Option<String>>,
    global_contract_hash: Option<Option<String>>,
}

#[derive(Debug, Default)]
struct AccountUpsertOutcome {
    first_seen: bool,
    reactivated: bool,
}

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
    stream_mode: NearModuleStreamMode,
}

impl AccountsModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        let (enabled, stream_mode) = match config {
            common::config::AppConfig::Indexer(idx) => {
                let idx = idx.as_ref();
                let near_config = idx.near.as_ref();

                // Check if NEAR indexing is enabled at all
                let near_enabled = near_config.map(|c| c.chain != "disabled").unwrap_or(false);

                if !near_enabled {
                    return Self {
                        enabled: false,
                        stream_mode: NearModuleStreamMode::Shared,
                    };
                }

                // Get accounts module configuration
                let module_config = near_config
                    .and_then(|c| c.modules.as_ref())
                    .and_then(|m| m.accounts.as_ref())
                    .cloned();

                (
                    module_config.as_ref().map(|a| a.enabled).unwrap_or(true),
                    module_config
                        .as_ref()
                        .map(|a| a.stream_mode)
                        .unwrap_or_default(),
                )
            }
            _ => (false, NearModuleStreamMode::Shared),
        };

        Self {
            enabled,
            stream_mode,
        }
    }

    /// Validate public key format
    fn validate_public_key(&self, public_key: &str) -> Result<()> {
        if public_key.is_empty() {
            return Err(anyhow::anyhow!("Public key cannot be empty"));
        }

        // NEAR keys are typically formatted as <curve>:<base58>
        let (curve, key_data) = if let Some((curve, key_data)) = public_key.split_once(':') {
            (Some(curve), key_data)
        } else {
            (None, public_key)
        };

        if let Some(curve) = curve {
            if curve != "ed25519" && curve != "secp256k1" {
                return Err(anyhow::anyhow!("Unsupported public key curve: {}", curve));
            }
        }

        // Basic length check for base58 payload
        if key_data.len() < 20 || key_data.len() > 128 {
            return Err(anyhow::anyhow!(
                "Public key has invalid length: {}",
                key_data.len()
            ));
        }

        // Validate base58 charset (no 0, O, I, l, or symbols)
        const BASE58_ALPHABET: &str = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
        for ch in key_data.chars() {
            if !BASE58_ALPHABET.contains(ch) {
                return Err(anyhow::anyhow!(
                    "Public key contains invalid base58 character: {}",
                    ch
                ));
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

    fn shard_to_i32(shard_id: ShardId) -> i32 {
        let value: u64 = shard_id.into();
        i32::try_from(value).unwrap_or(i32::MAX)
    }

    fn push_account_event(
        events: &mut Vec<AccountEventRecord>,
        account_id: String,
        event_type: &str,
        context: (Option<String>, Option<String>),
        metadata: serde_json::Value,
        location: (i32, i32),
        event_index: &mut i32,
    ) {
        let (transaction_hash, receipt_id) = context;
        let (shard_id, state_change_index) = location;

        events.push(AccountEventRecord {
            account_id,
            event_type: event_type.to_string(),
            transaction_hash,
            receipt_id,
            metadata,
            shard_id,
            state_change_index,
            event_index: *event_index,
        });
        *event_index = event_index.saturating_add(1);
    }

    async fn get_cached_account(
        &self,
        account_id: &str,
        db: &impl ConnectionTrait,
        account_cache: &mut HashMap<String, near_accounts::Model>,
    ) -> Result<Option<near_accounts::Model>> {
        if let Some(account) = account_cache.get(account_id) {
            return Ok(Some(account.clone()));
        }

        let account = near_accounts::Entity::find()
            .filter(near_accounts::Column::AccountId.eq(account_id))
            .one(db)
            .await?;

        if let Some(ref account_model) = account {
            account_cache.insert(account_id.to_string(), account_model.clone());
        }

        Ok(account)
    }

    async fn upsert_account_snapshot(
        &self,
        snapshot: AccountSnapshotUpsert,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
        account_cache: &mut HashMap<String, near_accounts::Model>,
    ) -> Result<AccountUpsertOutcome> {
        let account_type: String = classify_account(&snapshot.account_id).into();
        let parent_account = get_parent_account(&snapshot.account_id);

        let existing = self
            .get_cached_account(&snapshot.account_id, db, account_cache)
            .await?;

        if let Some(mut account) = existing {
            if block_height < account.last_seen_height {
                return Ok(AccountUpsertOutcome::default());
            }

            let mut outcome = AccountUpsertOutcome::default();
            let mut updated = false;

            if account.account_type != account_type {
                account.account_type = account_type;
                updated = true;
            }

            if account.parent_account_id != parent_account {
                account.parent_account_id = parent_account;
                updated = true;
            }

            if let Some(is_contract) = snapshot.is_contract {
                if account.is_contract != is_contract {
                    account.is_contract = is_contract;
                    updated = true;
                }
            }

            if let Some(global_contract_account_id) = snapshot.global_contract_account_id {
                if account.global_contract_account_id != global_contract_account_id {
                    account.global_contract_account_id = global_contract_account_id;
                    updated = true;
                }
            }

            if let Some(global_contract_hash) = snapshot.global_contract_hash {
                if account.global_contract_hash != global_contract_hash {
                    account.global_contract_hash = global_contract_hash;
                    updated = true;
                }
            }

            if snapshot.clear_deleted && account.deleted_at_height.is_some() {
                account.deleted_at_height = None;
                account.deleted_at_timestamp = None;
                outcome.reactivated = true;
                updated = true;
            }

            if account.last_seen_height != block_height {
                account.last_seen_height = block_height;
                updated = true;
            }

            if account.last_seen_timestamp != block_timestamp {
                account.last_seen_timestamp = block_timestamp;
                updated = true;
            }

            if updated {
                let active_model: near_accounts::ActiveModel = account.clone().into();
                active_model
                    .update(db)
                    .await
                    .context("Failed to update account snapshot")?;
            }

            account_cache.insert(snapshot.account_id.clone(), account);
            Ok(outcome)
        } else {
            let account = near_accounts::ActiveModel {
                account_id: Set(snapshot.account_id.clone()),
                account_type: Set(account_type),
                created_at_height: Set(block_height),
                created_at_timestamp: Set(block_timestamp),
                deleted_at_height: Set(None),
                deleted_at_timestamp: Set(None),
                parent_account_id: Set(parent_account),
                is_contract: Set(snapshot.is_contract.unwrap_or(false)),
                last_seen_height: Set(block_height),
                last_seen_timestamp: Set(block_timestamp),
                global_contract_account_id: Set(snapshot
                    .global_contract_account_id
                    .unwrap_or_default()),
                global_contract_hash: Set(snapshot.global_contract_hash.unwrap_or_default()),
            }
            .insert(db)
            .await
            .context("Failed to insert first-seen account snapshot")?;

            account_cache.insert(snapshot.account_id, account);

            Ok(AccountUpsertOutcome {
                first_seen: true,
                reactivated: false,
            })
        }
    }

    /// Process state changes to extract account-related data
    async fn process_state_changes(
        &self,
        shards: &[IndexerShard],
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
        account_cache: &mut HashMap<String, near_accounts::Model>,
    ) -> Result<()> {
        let mut account_balances = Vec::new();
        let mut account_events = Vec::new();

        for shard in shards {
            let shard_id = Self::shard_to_i32(shard.shard_id);

            for (change_index, state_change) in shard.state_changes.iter().enumerate() {
                let state_change_index = i32::try_from(change_index).unwrap_or(i32::MAX);
                let (tx_hash, receipt_hash) = self.extract_transaction_context(&state_change.cause);
                let mut event_index = 0;

                match &state_change.value {
                    StateChangeValueView::AccountUpdate {
                        account_id,
                        account,
                    } => {
                        let account_id_str = account_id.to_string();

                        if let Err(err) = validate_account_id(&account_id_str) {
                            trace!(account_id = %account_id_str, error = %err, "Invalid account ID format, skipping account update");
                            continue;
                        }

                        let global_contract_account_id = account
                            .global_contract_account_id
                            .as_ref()
                            .map(ToString::to_string);
                        let global_contract_hash = account
                            .global_contract_hash
                            .as_ref()
                            .map(ToString::to_string);
                        let is_contract = account.code_hash != CryptoHash::default()
                            || global_contract_account_id.is_some()
                            || global_contract_hash.is_some();

                        let upsert_outcome = self
                            .upsert_account_snapshot(
                                AccountSnapshotUpsert {
                                    account_id: account_id_str.clone(),
                                    is_contract: Some(is_contract),
                                    clear_deleted: true,
                                    global_contract_account_id: Some(global_contract_account_id),
                                    global_contract_hash: Some(global_contract_hash),
                                },
                                block_height,
                                block_timestamp,
                                db,
                                account_cache,
                            )
                            .await?;

                        if upsert_outcome.first_seen {
                            Self::push_account_event(
                                &mut account_events,
                                account_id_str.clone(),
                                "first_seen",
                                (tx_hash.clone(), receipt_hash.clone()),
                                serde_json::json!({
                                    "cause": state_change.cause,
                                    "observed_via": "account_update"
                                }),
                                (shard_id, state_change_index),
                                &mut event_index,
                            );
                        }

                        if upsert_outcome.reactivated {
                            Self::push_account_event(
                                &mut account_events,
                                account_id_str.clone(),
                                "reactivated",
                                (tx_hash.clone(), receipt_hash.clone()),
                                serde_json::json!({
                                    "cause": state_change.cause,
                                    "observed_via": "account_update"
                                }),
                                (shard_id, state_change_index),
                                &mut event_index,
                            );
                        }

                        account_balances.push(AccountBalanceUpdate {
                            account_id: account_id_str,
                            amount: account.amount.to_string(),
                            locked: account.locked.to_string(),
                            storage_usage: account.storage_usage as i64,
                            code_hash: account.code_hash.to_string(),
                            shard_id,
                            state_change_index,
                        });
                    }
                    StateChangeValueView::AccountDeletion { account_id } => {
                        let account_id_str = account_id.to_string();

                        if let Err(err) = validate_account_id(&account_id_str) {
                            trace!(account_id = %account_id_str, error = %err, "Invalid account ID format, skipping account deletion");
                            continue;
                        }

                        let upsert_outcome = self
                            .upsert_account_snapshot(
                                AccountSnapshotUpsert {
                                    account_id: account_id_str.clone(),
                                    is_contract: None,
                                    clear_deleted: false,
                                    global_contract_account_id: None,
                                    global_contract_hash: None,
                                },
                                block_height,
                                block_timestamp,
                                db,
                                account_cache,
                            )
                            .await?;

                        if upsert_outcome.first_seen {
                            Self::push_account_event(
                                &mut account_events,
                                account_id_str.clone(),
                                "first_seen",
                                (tx_hash.clone(), receipt_hash.clone()),
                                serde_json::json!({
                                    "cause": state_change.cause,
                                    "observed_via": "account_deletion"
                                }),
                                (shard_id, state_change_index),
                                &mut event_index,
                            );
                        }

                        self.mark_account_deleted(
                            &account_id_str,
                            block_height,
                            block_timestamp,
                            db,
                            account_cache,
                        )
                        .await
                        .context("Failed to mark account as deleted")?;

                        Self::push_account_event(
                            &mut account_events,
                            account_id_str,
                            "deleted",
                            (tx_hash, receipt_hash),
                            serde_json::json!({"cause": state_change.cause}),
                            (shard_id, state_change_index),
                            &mut event_index,
                        );
                    }
                    StateChangeValueView::AccessKeyUpdate {
                        account_id,
                        public_key,
                        access_key,
                    } => {
                        let account_id_str = account_id.to_string();
                        let public_key_str = public_key.to_string();

                        if let Err(err) = validate_account_id(&account_id_str) {
                            trace!(account_id = %account_id_str, error = %err, "Invalid account ID format for access key update, skipping");
                            continue;
                        }

                        if let Err(err) = self.validate_public_key(&public_key_str) {
                            trace!(account_id = %account_id_str, public_key = %public_key_str, error = %err, "Invalid public key format for access key update, skipping");
                            continue;
                        }

                        let upsert_outcome = self
                            .upsert_account_snapshot(
                                AccountSnapshotUpsert {
                                    account_id: account_id_str.clone(),
                                    is_contract: None,
                                    clear_deleted: false,
                                    global_contract_account_id: None,
                                    global_contract_hash: None,
                                },
                                block_height,
                                block_timestamp,
                                db,
                                account_cache,
                            )
                            .await?;

                        if upsert_outcome.first_seen {
                            Self::push_account_event(
                                &mut account_events,
                                account_id_str.clone(),
                                "first_seen",
                                (tx_hash.clone(), receipt_hash.clone()),
                                serde_json::json!({
                                    "cause": state_change.cause,
                                    "observed_via": "access_key_update"
                                }),
                                (shard_id, state_change_index),
                                &mut event_index,
                            );
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

                        self.upsert_access_key(
                            AccessKeyData {
                                account_id: account_id_str,
                                public_key: public_key_str,
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

                        if let Err(err) = validate_account_id(&account_id_str) {
                            trace!(account_id = %account_id_str, error = %err, "Invalid account ID format for access key deletion, skipping");
                            continue;
                        }

                        if let Err(err) = self.validate_public_key(&public_key_str) {
                            trace!(account_id = %account_id_str, public_key = %public_key_str, error = %err, "Invalid public key format for access key deletion, skipping");
                            continue;
                        }

                        let upsert_outcome = self
                            .upsert_account_snapshot(
                                AccountSnapshotUpsert {
                                    account_id: account_id_str.clone(),
                                    is_contract: None,
                                    clear_deleted: false,
                                    global_contract_account_id: None,
                                    global_contract_hash: None,
                                },
                                block_height,
                                block_timestamp,
                                db,
                                account_cache,
                            )
                            .await?;

                        if upsert_outcome.first_seen {
                            Self::push_account_event(
                                &mut account_events,
                                account_id_str.clone(),
                                "first_seen",
                                (tx_hash.clone(), receipt_hash.clone()),
                                serde_json::json!({
                                    "cause": state_change.cause,
                                    "observed_via": "access_key_deletion"
                                }),
                                (shard_id, state_change_index),
                                &mut event_index,
                            );
                        }

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
                        let account_id_str = account_id.to_string();

                        if let Err(err) = validate_account_id(&account_id_str) {
                            trace!(account_id = %account_id_str, error = %err, "Invalid account ID format for contract update, skipping");
                            continue;
                        }

                        let upsert_outcome = self
                            .upsert_account_snapshot(
                                AccountSnapshotUpsert {
                                    account_id: account_id_str.clone(),
                                    is_contract: Some(true),
                                    clear_deleted: true,
                                    global_contract_account_id: None,
                                    global_contract_hash: None,
                                },
                                block_height,
                                block_timestamp,
                                db,
                                account_cache,
                            )
                            .await?;

                        if upsert_outcome.first_seen {
                            Self::push_account_event(
                                &mut account_events,
                                account_id_str.clone(),
                                "first_seen",
                                (tx_hash.clone(), receipt_hash.clone()),
                                serde_json::json!({
                                    "cause": state_change.cause,
                                    "observed_via": "contract_code_update"
                                }),
                                (shard_id, state_change_index),
                                &mut event_index,
                            );
                        }

                        if upsert_outcome.reactivated {
                            Self::push_account_event(
                                &mut account_events,
                                account_id_str.clone(),
                                "reactivated",
                                (tx_hash.clone(), receipt_hash.clone()),
                                serde_json::json!({
                                    "cause": state_change.cause,
                                    "observed_via": "contract_code_update"
                                }),
                                (shard_id, state_change_index),
                                &mut event_index,
                            );
                        }

                        Self::push_account_event(
                            &mut account_events,
                            account_id_str,
                            "contract_updated",
                            (tx_hash, receipt_hash),
                            serde_json::json!({"cause": state_change.cause}),
                            (shard_id, state_change_index),
                            &mut event_index,
                        );
                    }
                    StateChangeValueView::ContractCodeDeletion { account_id } => {
                        let account_id_str = account_id.to_string();

                        if let Err(err) = validate_account_id(&account_id_str) {
                            trace!(account_id = %account_id_str, error = %err, "Invalid account ID format for contract deletion, skipping");
                            continue;
                        }

                        let upsert_outcome = self
                            .upsert_account_snapshot(
                                AccountSnapshotUpsert {
                                    account_id: account_id_str.clone(),
                                    is_contract: Some(false),
                                    clear_deleted: false,
                                    global_contract_account_id: None,
                                    global_contract_hash: None,
                                },
                                block_height,
                                block_timestamp,
                                db,
                                account_cache,
                            )
                            .await?;

                        if upsert_outcome.first_seen {
                            Self::push_account_event(
                                &mut account_events,
                                account_id_str.clone(),
                                "first_seen",
                                (tx_hash.clone(), receipt_hash.clone()),
                                serde_json::json!({
                                    "cause": state_change.cause,
                                    "observed_via": "contract_code_deletion"
                                }),
                                (shard_id, state_change_index),
                                &mut event_index,
                            );
                        }

                        Self::push_account_event(
                            &mut account_events,
                            account_id_str,
                            "contract_deleted",
                            (tx_hash, receipt_hash),
                            serde_json::json!({"cause": state_change.cause}),
                            (shard_id, state_change_index),
                            &mut event_index,
                        );
                    }
                    _ => {
                        // Other state changes (DataUpdate, DataDeletion)
                        // are not directly account-related for our purposes.
                    }
                }
            }
        }

        self.store_account_balances(account_balances, block_height, block_timestamp, db)
            .await?;
        self.store_account_events(account_events, block_height, block_timestamp, db)
            .await?;

        Ok(())
    }

    /// Store account balance updates
    async fn store_account_balances(
        &self,
        balances: Vec<AccountBalanceUpdate>,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        if balances.is_empty() {
            return Ok(());
        }

        let mut balance_records = Vec::with_capacity(balances.len());

        for balance in balances {
            let balance_record = near_account_balances::ActiveModel {
                id: sea_orm::ActiveValue::NotSet,
                account_id: Set(balance.account_id),
                block_height: Set(block_height),
                timestamp: Set(block_timestamp),
                amount: Set(balance.amount),
                locked: Set(balance.locked),
                storage_usage: Set(balance.storage_usage),
                code_hash: Set(balance.code_hash),
                shard_id: Set(balance.shard_id),
                state_change_index: Set(balance.state_change_index),
            };
            balance_records.push(balance_record);
        }

        near_account_balances::Entity::insert_many(balance_records)
            .on_conflict(
                OnConflict::columns([
                    near_account_balances::Column::BlockHeight,
                    near_account_balances::Column::ShardId,
                    near_account_balances::Column::StateChangeIndex,
                    near_account_balances::Column::AccountId,
                ])
                .do_nothing()
                .to_owned(),
            )
            .do_nothing()
            .exec(db)
            .await
            .context("Failed to insert account balance records")?;

        Ok(())
    }

    /// Store account events.
    async fn store_account_events(
        &self,
        events: Vec<AccountEventRecord>,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let mut event_records = Vec::with_capacity(events.len());

        for event in events {
            let event_record = near_account_events::ActiveModel {
                id: sea_orm::ActiveValue::NotSet,
                account_id: Set(event.account_id),
                event_type: Set(event.event_type),
                block_height: Set(block_height),
                timestamp: Set(block_timestamp),
                transaction_hash: Set(event.transaction_hash),
                receipt_id: Set(event.receipt_id),
                metadata: Set(Some(event.metadata)),
                shard_id: Set(event.shard_id),
                state_change_index: Set(event.state_change_index),
                event_index: Set(event.event_index),
            };
            event_records.push(event_record);
        }

        near_account_events::Entity::insert_many(event_records)
            .on_conflict(
                OnConflict::columns([
                    near_account_events::Column::BlockHeight,
                    near_account_events::Column::ShardId,
                    near_account_events::Column::StateChangeIndex,
                    near_account_events::Column::EventIndex,
                    near_account_events::Column::AccountId,
                ])
                .do_nothing()
                .to_owned(),
            )
            .do_nothing()
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
        account_cache: &mut HashMap<String, near_accounts::Model>,
    ) -> Result<()> {
        if let Some(mut account) = self
            .get_cached_account(account_id, db, account_cache)
            .await?
        {
            if block_height < account.last_seen_height {
                return Ok(());
            }

            let mut updated = false;

            if account.deleted_at_height != Some(block_height) {
                account.deleted_at_height = Some(block_height);
                updated = true;
            }

            if account.deleted_at_timestamp != Some(block_timestamp) {
                account.deleted_at_timestamp = Some(block_timestamp);
                updated = true;
            }

            if account.is_contract {
                account.is_contract = false;
                updated = true;
            }

            if account.global_contract_account_id.is_some() {
                account.global_contract_account_id = None;
                updated = true;
            }

            if account.global_contract_hash.is_some() {
                account.global_contract_hash = None;
                updated = true;
            }

            if account.last_seen_height != block_height {
                account.last_seen_height = block_height;
                updated = true;
            }

            if account.last_seen_timestamp != block_timestamp {
                account.last_seen_timestamp = block_timestamp;
                updated = true;
            }

            if updated {
                let active_model: near_accounts::ActiveModel = account.clone().into();
                active_model
                    .update(db)
                    .await
                    .context("Failed to update account deletion")?;
                debug!(
                    account_id = account_id,
                    height = block_height,
                    "Marked account as deleted"
                );
            }

            account_cache.insert(account_id.to_string(), account);
        } else {
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
        // Update current active key when possible.
        if let Some(existing_key) = near_access_keys::Entity::find()
            .filter(near_access_keys::Column::AccountId.eq(&key_data.account_id))
            .filter(near_access_keys::Column::PublicKey.eq(&key_data.public_key))
            .filter(near_access_keys::Column::IsActive.eq(true))
            .one(db)
            .await?
        {
            if key_data.block_height < existing_key.last_seen_height {
                return Ok(());
            }

            // Update existing key
            let mut existing_key: near_access_keys::ActiveModel = existing_key.into();
            existing_key.permission_type = Set(key_data.permission_type);
            existing_key.allowance = Set(key_data.allowance);
            existing_key.receiver_id = Set(key_data.receiver_id);
            existing_key.method_names = Set(key_data.method_names);
            existing_key.nonce = Set(key_data.nonce);
            existing_key.last_seen_height = Set(key_data.block_height);

            existing_key
                .update(db)
                .await
                .context("Failed to update access key")?;
            debug!(
                account_id = %key_data.account_id,
                public_key = %key_data.public_key,
                "Updated existing access key"
            );

            return Ok(());
        }

        // Guard against stale out-of-order writes when there is no active key.
        if let Some(latest_key) = near_access_keys::Entity::find()
            .filter(near_access_keys::Column::AccountId.eq(&key_data.account_id))
            .filter(near_access_keys::Column::PublicKey.eq(&key_data.public_key))
            .order_by_desc(near_access_keys::Column::LastSeenHeight)
            .order_by_desc(near_access_keys::Column::CreatedAtHeight)
            .order_by_desc(near_access_keys::Column::Id)
            .one(db)
            .await?
        {
            if key_data.block_height < latest_key.last_seen_height {
                return Ok(());
            }

            if key_data.block_height == latest_key.created_at_height {
                let mut latest_key: near_access_keys::ActiveModel = latest_key.into();
                latest_key.permission_type = Set(key_data.permission_type.clone());
                latest_key.allowance = Set(key_data.allowance.clone());
                latest_key.receiver_id = Set(key_data.receiver_id.clone());
                latest_key.method_names = Set(key_data.method_names.clone());
                latest_key.nonce = Set(key_data.nonce);
                latest_key.deleted_at_height = Set(None);
                latest_key.is_active = Set(true);
                latest_key.last_seen_height = Set(key_data.block_height);

                latest_key
                    .update(db)
                    .await
                    .context("Failed to reactivate access key lifecycle")?;

                debug!(
                    account_id = %key_data.account_id,
                    public_key = %key_data.public_key,
                    "Reactivated existing access key lifecycle"
                );

                return Ok(());
            }
        }

        // Insert new key. On replay, avoid duplicate rows for same key lifecycle start.
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
            last_seen_height: Set(key_data.block_height),
        };

        near_access_keys::Entity::insert_many([new_key])
            .on_conflict(
                OnConflict::columns([
                    near_access_keys::Column::AccountId,
                    near_access_keys::Column::PublicKey,
                    near_access_keys::Column::CreatedAtHeight,
                ])
                .do_nothing()
                .to_owned(),
            )
            .do_nothing()
            .exec(db)
            .await
            .context("Failed to insert new access key")?;

        debug!(
            account_id = %key_data.account_id,
            public_key = %key_data.public_key,
            "Inserted access key"
        );

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
            .filter(near_access_keys::Column::IsActive.eq(true))
            .one(db)
            .await?
        {
            if block_height < access_key.last_seen_height {
                return Ok(());
            }

            let mut access_key: near_access_keys::ActiveModel = access_key.into();
            access_key.deleted_at_height = Set(Some(block_height));
            access_key.is_active = Set(false);
            access_key.last_seen_height = Set(block_height);

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

    fn stream_mode(&self) -> NearModuleStreamMode {
        self.stream_mode
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
        let mut account_cache: HashMap<String, near_accounts::Model> = HashMap::new();

        self.process_state_changes(
            &msg.shards,
            block_height,
            block_timestamp,
            &txn,
            &mut account_cache,
        )
        .await
        .context("Failed to process state changes for accounts")?;

        txn.commit().await?;

        let elapsed = start_time.elapsed();
        info!(
            height = block_height,
            accounts_processed = account_cache.len(),
            elapsed_ms = elapsed.as_millis(),
            "Successfully processed accounts for block"
        );

        Ok(())
    }

    async fn get_cursor(&self, db: &DatabaseConnection) -> Result<Option<i64>> {
        cursor::get_module_cursor(db, MODULE_NAME).await
    }

    async fn update_cursor(
        &self,
        db: &DatabaseConnection,
        height: i64,
        prev_height: Option<i64>,
    ) -> Result<()> {
        cursor::upsert_module_cursor(db, MODULE_NAME, height, prev_height).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module() -> AccountsModule {
        AccountsModule {
            enabled: true,
            stream_mode: NearModuleStreamMode::Shared,
        }
    }

    #[test]
    fn validate_public_key_accepts_valid_near_keys() {
        let accounts_module = module();

        assert!(accounts_module
            .validate_public_key("ed25519:5ZE7dM8jkVdwf2EgYwjY2JgUUhzj4xwNubBca8h2P6Hp")
            .is_ok());
        assert!(accounts_module
            .validate_public_key("secp256k1:4NfTiv6b2wtqP5MBoYQ6nQ6QJ6w6V4Y5y6KLdM9AbCdE")
            .is_ok());
    }

    #[test]
    fn validate_public_key_rejects_invalid_base58_chars() {
        let accounts_module = module();

        assert!(accounts_module
            .validate_public_key("ed25519:abc0defghijklmnopqrstuvwxy")
            .is_err());
        assert!(accounts_module
            .validate_public_key("ed25519:abc*defghijklmnopqrstuvwxy")
            .is_err());
    }

    #[test]
    fn validate_public_key_rejects_unknown_curve() {
        let accounts_module = module();

        assert!(accounts_module
            .validate_public_key("rsa:5ZE7dM8jkVdwf2EgYwjY2JgUUhzj4xwNubBca8h2P6Hp")
            .is_err());
    }
}
