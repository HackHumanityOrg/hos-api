use anyhow::Result;
use async_trait::async_trait;
use near_account_id::AccountIdRef;
use near_lake_framework::near_indexer_primitives::views::{
    ActionView, ReceiptEnumView, StateChangeCauseView, StateChangeValueView,
};
use sea_orm::sea_query::OnConflict;
use sea_orm::ActiveValue::NotSet;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set, TransactionTrait};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::{sync::Semaphore, task::JoinSet};
use tracing::debug;

use crate::core::cursor;
use crate::IndexerModule;

const MODULE_NAME: &str = "accounts";

/// Accounts indexer module
pub struct AccountsModule {
    enabled: bool,
    concurrency: usize,
}

impl AccountsModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        let (enabled, concurrency) = match config {
            common::config::AppConfig::Indexer(idx) => {
                let near_config = idx.near.as_ref();

                // Check if NEAR indexing is enabled at all
                let near_enabled = near_config.map(|c| c.chain != "disabled").unwrap_or(false);

                if !near_enabled {
                    return Self {
                        enabled: false,
                        concurrency: 1,
                    };
                }

                let concurrency = near_config.and_then(|c| c.concurrency).unwrap_or(1);

                // Get accounts module configuration
                let accounts_config = near_config
                    .and_then(|nc| nc.modules.as_ref())
                    .and_then(|mc| mc.accounts.as_ref());

                let enabled = accounts_config.map(|ac| ac.enabled).unwrap_or(true); // Default to enabled for backward compatibility

                (enabled, concurrency)
            }
            _ => (false, 1),
        };

        Self {
            enabled,
            concurrency,
        }
    }

    /// Extract a map of deleted account -> beneficiary for the entire block.
    fn pre_scan_delete_beneficiaries(
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
    ) -> HashMap<String, String> {
        let mut map = HashMap::new();
        for shard in &msg.shards {
            for reo in &shard.receipt_execution_outcomes {
                if let ReceiptEnumView::Action { actions, .. } = &reo.receipt.receipt {
                    let receiver_id = reo.receipt.receiver_id.clone();
                    for action in actions {
                        if let ActionView::DeleteAccount { beneficiary_id } = action {
                            map.insert(receiver_id.to_string(), beneficiary_id.to_string());
                        }
                    }
                }
            }
        }
        map
    }

    async fn process_shard(
        db: &DatabaseConnection,
        shard: near_lake_framework::near_indexer_primitives::IndexerShard,
        height: i64,
        ts: i64,
        delete_beneficiaries: &HashMap<String, String>,
    ) -> Result<()> {
        let txn = db.begin().await?;

        // Accumulate rows for batch operations
        let mut account_upserts_updates: Vec<(i32, entities::near_accounts::ActiveModel)> =
            Vec::new();
        let mut account_upserts_deletes: Vec<(i32, entities::near_accounts::ActiveModel)> =
            Vec::new();
        let mut state_history_inserts: Vec<entities::near_account_state_history::ActiveModel> =
            Vec::new();
        let mut event_inserts_receipt: Vec<entities::near_account_events::ActiveModel> = Vec::new();

        for sc in shard.state_changes {
            let (cause_str, cause_tx, cause_receipt) = match &sc.cause {
                StateChangeCauseView::NotWritableToDisk => ("not_writable_to_disk", None, None),
                StateChangeCauseView::InitialState => ("initial_state", None, None),
                StateChangeCauseView::TransactionProcessing { tx_hash } => {
                    ("transaction_processing", Some(tx_hash.to_string()), None)
                }
                StateChangeCauseView::ActionReceiptProcessingStarted { receipt_hash } => (
                    "action_receipt_processing_started",
                    None,
                    Some(receipt_hash.to_string()),
                ),
                StateChangeCauseView::ActionReceiptGasReward { receipt_hash } => (
                    "action_receipt_gas_reward",
                    None,
                    Some(receipt_hash.to_string()),
                ),
                StateChangeCauseView::ReceiptProcessing { receipt_hash } => {
                    ("receipt_processing", None, Some(receipt_hash.to_string()))
                }
                StateChangeCauseView::PostponedReceipt { receipt_hash } => {
                    ("postponed_receipt", None, Some(receipt_hash.to_string()))
                }
                StateChangeCauseView::UpdatedDelayedReceipts => {
                    ("updated_delayed_receipts", None, None)
                }
                StateChangeCauseView::ValidatorAccountsUpdate => {
                    ("validator_accounts_update", None, None)
                }
                StateChangeCauseView::Migration => ("migration", None, None),
                StateChangeCauseView::BandwidthSchedulerStateUpdate => {
                    ("bandwidth_scheduler_state_update", None, None)
                }
            };

            match sc.value {
                StateChangeValueView::AccountUpdate {
                    account_id,
                    account,
                } => {
                    let acc_str = account_id.to_string();

                    // Hierarchy and classification via near-account-id
                    let aid = AccountIdRef::new(&acc_str).ok();
                    let (parent, depth, is_top_level, is_system, is_implicit) = if let Some(a) = aid
                    {
                        let parent = a.get_parent_account_id().map(|p| p.as_str().to_string());
                        let depth = if acc_str.is_empty() {
                            0
                        } else {
                            acc_str.matches('.').count() as i32 + 1
                        };
                        let is_top_level = a.is_top_level();
                        let is_system = a.is_system();
                        let is_implicit = a.get_account_type().is_implicit();
                        (parent, depth, is_top_level, is_system, is_implicit)
                    } else {
                        (None, 0, false, false, false)
                    };

                    // Prepare near_accounts upsert row
                    let model = entities::near_accounts::ActiveModel {
                        account_id: Set(acc_str.clone()),
                        parent_account_id: Set(parent.clone()),
                        depth: Set(depth),
                        is_top_level: Set(if is_top_level { 1 } else { 0 }),
                        is_implicit: Set(if is_implicit { 1 } else { 0 }),
                        is_system: Set(if is_system { 1 } else { 0 }),
                        created_at_height: Set(Some(height)),
                        created_at_time: Set(Some(ts)),
                        created_cause: Set(Some(cause_str.to_string())),
                        created_by_tx_hash: Set(cause_tx.clone()),
                        created_by_receipt_hash: Set(cause_receipt.clone()),
                        deleted_at_height: Set(None),
                        deleted_at_time: Set(None),
                        deleted_beneficiary_id: Set(None),
                        deleted_by_tx_hash: Set(None),
                        deleted_by_receipt_hash: Set(None),
                        first_seen_height: Set(Some(height)),
                        first_seen_time: Set(Some(ts)),
                        last_seen_height: Set(Some(height)),
                        last_seen_time: Set(Some(ts)),
                    };
                    account_upserts_updates.push((depth, model));

                    // Insert full state history if unique
                    let code_hash = account.code_hash.to_string();
                    let amount_dec =
                        rust_decimal::Decimal::from_str_exact(&account.amount.to_string())
                            .unwrap_or(rust_decimal::Decimal::ZERO);
                    let locked_dec =
                        rust_decimal::Decimal::from_str_exact(&account.locked.to_string())
                            .unwrap_or(rust_decimal::Decimal::ZERO);
                    let storage_usage = account.storage_usage as i64;

                    let history = entities::near_account_state_history::ActiveModel {
                        id: NotSet,
                        account_id: Set(acc_str.clone()),
                        block_height: Set(height),
                        block_time: Set(ts),
                        amount: Set(amount_dec),
                        locked: Set(locked_dec),
                        storage_usage: Set(storage_usage),
                        code_hash: Set(code_hash),
                        cause_type: Set(cause_str.to_string()),
                        cause_tx_hash: Set(cause_tx.clone()),
                        cause_receipt_hash: Set(cause_receipt.clone()),
                    };
                    state_history_inserts.push(history);
                }
                StateChangeValueView::AccountDeletion { account_id } => {
                    let acc_str = account_id.to_string();
                    let beneficiary = delete_beneficiaries.get(&acc_str).cloned();
                    let model = entities::near_accounts::ActiveModel {
                        account_id: Set(acc_str.clone()),
                        parent_account_id: Set(None),
                        depth: Set(0),
                        is_top_level: Set(0),
                        is_implicit: Set(0),
                        is_system: Set(0),
                        created_at_height: Set(None),
                        created_at_time: Set(None),
                        created_cause: Set(None),
                        created_by_tx_hash: Set(None),
                        created_by_receipt_hash: Set(None),
                        deleted_at_height: Set(Some(height)),
                        deleted_at_time: Set(Some(ts)),
                        deleted_beneficiary_id: Set(beneficiary.clone()),
                        deleted_by_tx_hash: Set(cause_tx.clone()),
                        deleted_by_receipt_hash: Set(cause_receipt.clone()),
                        first_seen_height: Set(Some(height)),
                        first_seen_time: Set(Some(ts)),
                        last_seen_height: Set(Some(height)),
                        last_seen_time: Set(Some(ts)),
                    };
                    account_upserts_deletes.push((0, model));

                    let meta = json!({ "beneficiary_id": beneficiary }).to_string();
                    let has_receipt = cause_receipt.is_some();
                    let evt = entities::near_account_events::ActiveModel {
                        id: NotSet,
                        account_id: Set(acc_str.clone()),
                        event_type: Set("delete".to_string()),
                        block_height: Set(height),
                        block_time: Set(ts),
                        tx_hash: Set(cause_tx.clone()),
                        receipt_hash: Set(cause_receipt.clone()),
                        metadata: Set(Some(meta)),
                    };
                    if has_receipt {
                        event_inserts_receipt.push(evt);
                    } else {
                        // Fallback: perform select+insert for NULL receipt_hash case
                        let exists = entities::near_account_events::Entity::find()
                            .filter(
                                entities::near_account_events::Column::AccountId
                                    .eq(acc_str.clone()),
                            )
                            .filter(entities::near_account_events::Column::EventType.eq("delete"))
                            .filter(entities::near_account_events::Column::BlockHeight.eq(height))
                            .filter(entities::near_account_events::Column::ReceiptHash.is_null())
                            .one(&txn)
                            .await?;
                        if exists.is_none() {
                            let _ = entities::near_account_events::Entity::insert(evt)
                                .exec(&txn)
                                .await?;
                        }
                    }
                }
                _ => {}
            }
        }

        // Batch upserts for account updates
        if !account_upserts_updates.is_empty() {
            use entities::near_accounts::Column as AC;
            account_upserts_updates.sort_by_key(|(d, _)| *d);
            let mut i = 0;
            while i < account_upserts_updates.len() {
                let depth = account_upserts_updates[i].0;
                let mut j = i;
                while j < account_upserts_updates.len() && account_upserts_updates[j].0 == depth {
                    j += 1;
                }
                let batch: Vec<entities::near_accounts::ActiveModel> = account_upserts_updates
                    [i..j]
                    .iter()
                    .map(|(_, m)| m.clone())
                    .collect();
                entities::near_accounts::Entity::insert_many(batch)
                    .on_conflict(
                        OnConflict::column(AC::AccountId)
                            .update_columns([
                                AC::ParentAccountId,
                                AC::Depth,
                                AC::IsTopLevel,
                                AC::IsImplicit,
                                AC::IsSystem,
                                AC::LastSeenHeight,
                                AC::LastSeenTime,
                            ])
                            .to_owned(),
                    )
                    .exec(&txn)
                    .await?;
                i = j;
            }
        }

        // Batch upserts for account deletions
        if !account_upserts_deletes.is_empty() {
            use entities::near_accounts::Column as AC;
            account_upserts_deletes.sort_by_key(|(d, _)| *d);
            let mut i = 0;
            while i < account_upserts_deletes.len() {
                let depth = account_upserts_deletes[i].0;
                let mut j = i;
                while j < account_upserts_deletes.len() && account_upserts_deletes[j].0 == depth {
                    j += 1;
                }
                let batch: Vec<entities::near_accounts::ActiveModel> = account_upserts_deletes
                    [i..j]
                    .iter()
                    .map(|(_, m)| m.clone())
                    .collect();
                entities::near_accounts::Entity::insert_many(batch)
                    .on_conflict(
                        OnConflict::column(AC::AccountId)
                            .update_columns([
                                AC::DeletedAtHeight,
                                AC::DeletedAtTime,
                                AC::DeletedBeneficiaryId,
                                AC::DeletedByTxHash,
                                AC::DeletedByReceiptHash,
                                AC::LastSeenHeight,
                                AC::LastSeenTime,
                            ])
                            .to_owned(),
                    )
                    .exec(&txn)
                    .await?;
                i = j;
            }
        }

        // Batch insert for state history
        if !state_history_inserts.is_empty() {
            entities::near_account_state_history::Entity::insert_many(state_history_inserts)
                .on_conflict_do_nothing()
                .exec(&txn)
                .await?;
        }

        // Batch insert for events with receipt hash
        if !event_inserts_receipt.is_empty() {
            entities::near_account_events::Entity::insert_many(event_inserts_receipt)
                .on_conflict_do_nothing()
                .exec(&txn)
                .await?;
        }

        txn.commit().await?;
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

    async fn process_block(
        &self,
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()> {
        let height = msg.block.header.height as i64;
        let ts = msg.block.header.timestamp_nanosec as i64;

        let delete_beneficiaries = Arc::new(Self::pre_scan_delete_beneficiaries(msg));

        // Process shards with concurrency
        let max_conc = if self.concurrency == 0 {
            msg.shards.len().max(1)
        } else {
            self.concurrency.max(1)
        };
        let semaphore = Arc::new(Semaphore::new(max_conc));
        let mut join_set = JoinSet::new();

        for shard in msg.shards.clone() {
            let permit = semaphore.clone().acquire_owned().await.expect("semaphore");
            let db_clone = db.clone();
            let del_map = delete_beneficiaries.clone();
            join_set.spawn(async move {
                let _permit = permit;
                Self::process_shard(&db_clone, shard, height, ts, &del_map).await
            });
        }

        while let Some(res) = join_set.join_next().await {
            res??;
        }

        debug!(height, module = MODULE_NAME, "processed block");
        Ok(())
    }

    async fn get_cursor(&self, db: &DatabaseConnection) -> Result<Option<i64>> {
        cursor::get_module_cursor(db, MODULE_NAME).await
    }

    async fn update_cursor(&self, db: &DatabaseConnection, height: i64) -> Result<()> {
        cursor::upsert_module_cursor(db, MODULE_NAME, height).await
    }
}
