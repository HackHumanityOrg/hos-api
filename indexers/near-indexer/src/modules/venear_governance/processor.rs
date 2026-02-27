use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use base64::Engine;
use near_lake_framework::near_indexer_primitives::{
    views::{ActionView, ReceiptEnumView, ReceiptView},
    IndexerShard, StreamerMessage,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::{json, Value as JsonValue};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tracing::{info, instrument, warn};

use crate::core::cursor;
use crate::modules::venear_governance::types::{
    apply_balance_delta, derive_lockup_account_id, is_outcome_success, json_string, json_u128,
    json_u64, parse_args_json, parse_event_log, parse_i32_field,
};
use crate::IndexerModule;

const MODULE_NAME: &str = "venear_governance";

#[derive(Default)]
struct ProcessState {
    known_lockups: HashSet<String>,
    lockup_owners: HashMap<String, String>,
    account_balances: HashMap<String, u128>,
}

#[derive(Clone)]
struct ActionContext {
    predecessor_id: String,
    receiver_id: String,
    transaction_hash: Option<String>,
    receipt_id: String,
    execution_outcome_id: String,
    block_height: i64,
    block_timestamp: i64,
    shard_id: i32,
    success: bool,
}

pub struct VenearGovernanceModule {
    enabled: bool,
    venear_contract_id: String,
    voting_contract_id: String,
    lockup_account_suffix: String,
    configured_known_lockups: HashSet<String>,
    known_lockups: Arc<RwLock<HashSet<String>>>,
    lockup_owners: Arc<RwLock<HashMap<String, String>>>,
}

impl VenearGovernanceModule {
    fn shard_to_i32(shard_id: near_lake_framework::near_indexer_primitives::types::ShardId) -> i32 {
        let value: u64 = shard_id.into();
        i32::try_from(value).unwrap_or(i32::MAX)
    }

    pub fn new(config: &common::config::AppConfig) -> Self {
        let mut enabled = false;
        let mut venear_contract_id = "venear.dao".to_string();
        let mut voting_contract_id = "vote.dao".to_string();
        let mut lockup_account_suffix = ".venear.dao".to_string();
        let mut configured_known_lockups: HashSet<String> = HashSet::new();

        if let common::config::AppConfig::Indexer(idx) = config {
            let idx = idx.as_ref();
            if let Some(near) = idx.near.as_ref() {
                let near_enabled = near.chain != "disabled";
                if near_enabled {
                    if let Some(module_cfg) = near
                        .modules
                        .as_ref()
                        .and_then(|m| m.venear_governance.as_ref())
                    {
                        enabled = module_cfg.enabled;
                        venear_contract_id = module_cfg.venear_contract_id.clone();
                        voting_contract_id = module_cfg.voting_contract_id.clone();
                        lockup_account_suffix = module_cfg.lockup_account_suffix.clone();
                        configured_known_lockups =
                            module_cfg.known_lockups.iter().cloned().collect();
                    }
                }
            }
        }

        Self {
            enabled,
            venear_contract_id,
            voting_contract_id,
            lockup_account_suffix,
            configured_known_lockups,
            known_lockups: Arc::new(RwLock::new(HashSet::new())),
            lockup_owners: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn load_known_lockups(&self, db: &DatabaseConnection) -> Result<()> {
        let rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT lockup_account_id, owner_account_id, is_active FROM near.near_venear_lockups",
                vec![],
            ))
            .await
            .context("failed loading known lockups")?;

        let mut known_lockups = self.known_lockups.write().unwrap();
        let mut lockup_owners = self.lockup_owners.write().unwrap();

        known_lockups.clear();
        lockup_owners.clear();

        for lockup in &self.configured_known_lockups {
            known_lockups.insert(lockup.clone());
        }

        for row in rows {
            let lockup_account_id: String = row.try_get("", "lockup_account_id")?;
            let owner_account_id: String = row.try_get("", "owner_account_id")?;
            let is_active: bool = row.try_get("", "is_active")?;

            if is_active {
                known_lockups.insert(lockup_account_id.clone());
            }
            lockup_owners.insert(lockup_account_id, owner_account_id);
        }

        Ok(())
    }

    fn build_process_state(&self) -> ProcessState {
        let mut known_lockups = self.known_lockups.read().unwrap().clone();
        for lockup in &self.configured_known_lockups {
            known_lockups.insert(lockup.clone());
        }

        ProcessState {
            known_lockups,
            lockup_owners: self.lockup_owners.read().unwrap().clone(),
            account_balances: HashMap::new(),
        }
    }

    fn merge_process_state(&self, state: &ProcessState) {
        {
            let mut known_lockups = self.known_lockups.write().unwrap();
            for lockup in &state.known_lockups {
                known_lockups.insert(lockup.clone());
            }
        }

        {
            let mut lockup_owners = self.lockup_owners.write().unwrap();
            for (lockup, owner) in &state.lockup_owners {
                lockup_owners.insert(lockup.clone(), owner.clone());
            }
        }
    }

    fn is_target_contract(&self, contract_id: &str, state: &ProcessState) -> bool {
        contract_id == self.venear_contract_id
            || contract_id == self.voting_contract_id
            || state.known_lockups.contains(contract_id)
    }

    async fn process_shard(
        &self,
        shard: &IndexerShard,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
        state: &mut ProcessState,
    ) -> Result<()> {
        if let Some(chunk) = &shard.chunk {
            let shard_id_i32 = Self::shard_to_i32(shard.shard_id);
            for tx_with_outcome in &chunk.transactions {
                let tx = &tx_with_outcome.transaction;
                let outcome = &tx_with_outcome.outcome.execution_outcome;
                let status_json =
                    serde_json::to_value(&outcome.outcome.status).unwrap_or(JsonValue::Null);
                let success = is_outcome_success(&status_json);

                self.process_logs(
                    &outcome.outcome.logs,
                    tx.receiver_id.as_ref(),
                    tx.signer_id.as_ref(),
                    &outcome.id.to_string(),
                    &outcome.id.to_string(),
                    shard_id_i32,
                    block_height,
                    block_timestamp,
                    success,
                    db,
                    state,
                )
                .await?;

                let action_context = ActionContext {
                    predecessor_id: tx.signer_id.to_string(),
                    receiver_id: tx.receiver_id.to_string(),
                    transaction_hash: Some(tx.hash.to_string()),
                    receipt_id: outcome.id.to_string(),
                    execution_outcome_id: outcome.id.to_string(),
                    block_height,
                    block_timestamp,
                    shard_id: shard_id_i32,
                    success,
                };

                self.process_actions(&tx.actions, &action_context, db, state)
                    .await?;
            }
        }

        for receipt_with_outcome in &shard.receipt_execution_outcomes {
            let shard_id_i32 = Self::shard_to_i32(shard.shard_id);
            let receipt = &receipt_with_outcome.receipt;
            let outcome = &receipt_with_outcome.execution_outcome;
            let status_json =
                serde_json::to_value(&outcome.outcome.status).unwrap_or(JsonValue::Null);
            let success = is_outcome_success(&status_json);

            self.process_logs(
                &outcome.outcome.logs,
                receipt.receiver_id.as_ref(),
                receipt.predecessor_id.as_ref(),
                &receipt.receipt_id.to_string(),
                &outcome.id.to_string(),
                shard_id_i32,
                block_height,
                block_timestamp,
                success,
                db,
                state,
            )
            .await?;

            if let Some(actions) = Self::extract_actions_from_receipt(receipt) {
                let action_context = ActionContext {
                    predecessor_id: receipt.predecessor_id.to_string(),
                    receiver_id: receipt.receiver_id.to_string(),
                    transaction_hash: None,
                    receipt_id: receipt.receipt_id.to_string(),
                    execution_outcome_id: outcome.id.to_string(),
                    block_height,
                    block_timestamp,
                    shard_id: shard_id_i32,
                    success,
                };

                self.process_actions(&actions, &action_context, db, state)
                    .await?;
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_logs(
        &self,
        logs: &[String],
        contract_id: &str,
        predecessor_id: &str,
        receipt_id: &str,
        execution_outcome_id: &str,
        shard_id: i32,
        block_height: i64,
        block_timestamp: i64,
        success: bool,
        db: &impl ConnectionTrait,
        state: &mut ProcessState,
    ) -> Result<()> {
        if !self.is_target_contract(contract_id, state) {
            return Ok(());
        }

        for (log_index, log) in logs.iter().enumerate() {
            if !log.starts_with(crate::modules::venear_governance::types::EVENT_JSON_PREFIX) {
                continue;
            }

            let parsed = parse_event_log(log);
            if let Some(parse_error) = parsed.parse_error.as_ref() {
                self.upsert_raw_event(
                    db,
                    block_height,
                    block_timestamp,
                    shard_id,
                    receipt_id,
                    execution_outcome_id,
                    predecessor_id,
                    contract_id,
                    log_index as i32,
                    0,
                    None,
                    None,
                    success,
                    Some(parse_error.clone()),
                    None,
                    log,
                )
                .await?;
                continue;
            }

            if parsed.data_items.is_empty() {
                self.upsert_raw_event(
                    db,
                    block_height,
                    block_timestamp,
                    shard_id,
                    receipt_id,
                    execution_outcome_id,
                    predecessor_id,
                    contract_id,
                    log_index as i32,
                    0,
                    parsed.standard.as_deref(),
                    parsed.event_type.as_deref(),
                    success,
                    None,
                    None,
                    log,
                )
                .await?;
                continue;
            }

            for (data_index, data_item) in parsed.data_items.iter().enumerate() {
                self.upsert_raw_event(
                    db,
                    block_height,
                    block_timestamp,
                    shard_id,
                    receipt_id,
                    execution_outcome_id,
                    predecessor_id,
                    contract_id,
                    log_index as i32,
                    data_index as i32,
                    parsed.standard.as_deref(),
                    parsed.event_type.as_deref(),
                    success,
                    None,
                    Some(data_item.clone()),
                    log,
                )
                .await?;

                if !success {
                    continue;
                }

                self.normalize_event(
                    db,
                    state,
                    parsed.standard.as_deref(),
                    parsed.event_type.as_deref(),
                    data_item,
                    block_height,
                    block_timestamp,
                    shard_id,
                    receipt_id,
                    execution_outcome_id,
                    predecessor_id,
                    contract_id,
                    log_index as i32,
                    data_index as i32,
                )
                .await?;
            }
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn normalize_event(
        &self,
        db: &impl ConnectionTrait,
        state: &mut ProcessState,
        standard: Option<&str>,
        event_type: Option<&str>,
        data: &JsonValue,
        block_height: i64,
        block_timestamp: i64,
        shard_id: i32,
        receipt_id: &str,
        execution_outcome_id: &str,
        predecessor_id: &str,
        contract_id: &str,
        log_index: i32,
        data_index: i32,
    ) -> Result<()> {
        let (Some(standard), Some(event_type)) = (standard, event_type) else {
            return Ok(());
        };

        match (standard, event_type) {
            ("venear", "lockup_deployed") => {
                if let Some(owner_account_id) = json_string(data, "account_id") {
                    let lockup_account_id =
                        derive_lockup_account_id(&owner_account_id, &self.lockup_account_suffix);
                    let lockup_version = json_u64(data, "lockup_version").map(|v| v as i64);
                    self.upsert_lockup(
                        db,
                        &lockup_account_id,
                        &owner_account_id,
                        lockup_version,
                        "lockup_deployed_event",
                        block_height,
                        block_timestamp,
                        true,
                        None,
                    )
                    .await?;
                    state.known_lockups.insert(lockup_account_id.clone());
                    state
                        .lockup_owners
                        .insert(lockup_account_id, owner_account_id.clone());
                }
            }
            ("venear", "lockup_delete") => {
                if let Some(owner_account_id) = json_string(data, "account_id") {
                    let lockup_account_id =
                        derive_lockup_account_id(&owner_account_id, &self.lockup_account_suffix);
                    let lockup_version = json_u64(data, "lockup_version").map(|v| v as i64);
                    self.upsert_lockup(
                        db,
                        &lockup_account_id,
                        &owner_account_id,
                        lockup_version,
                        "lockup_delete_event",
                        block_height,
                        block_timestamp,
                        false,
                        Some(block_height),
                    )
                    .await?;
                    state.known_lockups.remove(&lockup_account_id);
                    state
                        .lockup_owners
                        .insert(lockup_account_id, owner_account_id.clone());
                }
            }
            ("venear", "lockup_update") => {
                if let Some(account_id) = json_string(data, "account_id") {
                    let existing_total = self
                        .get_existing_balance_total_for_identity(
                            db,
                            execution_outcome_id,
                            log_index,
                            data_index,
                            &account_id,
                        )
                        .await?;

                    let total_balance = if let Some(existing_total) = existing_total {
                        existing_total
                    } else {
                        self.get_latest_total_balance(db, state, &account_id)
                            .await?
                    };

                    let lockup_update_nonce =
                        json_u64(data, "lockup_update_nonce").map(|v| v as i64);
                    let lockup_version = json_u64(data, "lockup_version").map(|v| v as i64);
                    let locked_near_balance = json_u128(data, "locked_near_balance");

                    self.upsert_account_balance_row(
                        db,
                        &account_id,
                        "lockup_update",
                        None,
                        total_balance,
                        locked_near_balance,
                        lockup_update_nonce,
                        lockup_version,
                        block_height,
                        block_timestamp,
                        shard_id,
                        receipt_id,
                        execution_outcome_id,
                        predecessor_id,
                        contract_id,
                        log_index,
                        data_index,
                        Some(data.clone()),
                    )
                    .await?;
                    state.account_balances.insert(account_id, total_balance);
                }
            }
            ("venear", "create_proposal")
            | ("venear", "proposal_approve")
            | ("venear", "proposal_reject") => {
                let Some(proposal_id_raw) = json_u64(data, "proposal_id") else {
                    return Ok(());
                };

                let proposal_id = i64::try_from(proposal_id_raw)
                    .map_err(|_| anyhow!("proposal_id out of i64 range"))?;

                let proposer_id = json_string(data, "proposer_id");
                let account_id = json_string(data, "account_id");
                let voting_start_time_sec =
                    json_u64(data, "voting_start_time_sec").map(|v| v as i64);

                self.upsert_proposal_event(
                    db,
                    proposal_id,
                    event_type,
                    account_id,
                    proposer_id,
                    None,
                    voting_start_time_sec,
                    None,
                    None,
                    None,
                    None,
                    Some(data.clone()),
                    block_height,
                    block_timestamp,
                    shard_id,
                    receipt_id,
                    execution_outcome_id,
                    predecessor_id,
                    contract_id,
                    log_index,
                    data_index,
                )
                .await?;
            }
            ("venear", "add_vote") | ("venear", "remove_vote") => {
                let account_id = match json_string(data, "account_id") {
                    Some(account) => account,
                    None => return Ok(()),
                };

                let Some(proposal_id_raw) = json_u64(data, "proposal_id") else {
                    return Ok(());
                };
                let proposal_id = i64::try_from(proposal_id_raw)
                    .map_err(|_| anyhow!("proposal_id out of i64 range"))?;
                let vote = parse_i32_field(data, "vote")?;
                let account_balance = json_u128(data, "account_balance")
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "0".to_string());

                self.upsert_vote_event(
                    db,
                    proposal_id,
                    &account_id,
                    vote,
                    &account_balance,
                    event_type,
                    Some(data.clone()),
                    block_height,
                    block_timestamp,
                    shard_id,
                    receipt_id,
                    execution_outcome_id,
                    predecessor_id,
                    contract_id,
                    log_index,
                    data_index,
                )
                .await?;
            }
            ("nep141", "ft_mint") | ("nep141", "ft_burn")
                if contract_id == self.venear_contract_id =>
            {
                let owner_id = match json_string(data, "owner_id") {
                    Some(owner) => owner,
                    None => return Ok(()),
                };
                let amount = match json_u128(data, "amount") {
                    Some(amount) => amount,
                    None => return Ok(()),
                };

                let existing_total = self
                    .get_existing_balance_total_for_identity(
                        db,
                        execution_outcome_id,
                        log_index,
                        data_index,
                        &owner_id,
                    )
                    .await?;

                let total_balance = if let Some(existing_total) = existing_total {
                    existing_total
                } else {
                    let previous_balance =
                        self.get_latest_total_balance(db, state, &owner_id).await?;
                    apply_balance_delta(previous_balance, event_type, amount)
                };

                let delta_amount = if event_type == "ft_mint" {
                    amount.to_string()
                } else {
                    format!("-{amount}")
                };

                self.upsert_account_balance_row(
                    db,
                    &owner_id,
                    event_type,
                    Some(delta_amount),
                    total_balance,
                    None,
                    None,
                    None,
                    block_height,
                    block_timestamp,
                    shard_id,
                    receipt_id,
                    execution_outcome_id,
                    predecessor_id,
                    contract_id,
                    log_index,
                    data_index,
                    Some(data.clone()),
                )
                .await?;

                state.account_balances.insert(owner_id, total_balance);
            }
            _ => {}
        }

        Ok(())
    }

    async fn process_actions(
        &self,
        actions: &[ActionView],
        context: &ActionContext,
        db: &impl ConnectionTrait,
        state: &mut ProcessState,
    ) -> Result<()> {
        for (action_index, action) in actions.iter().enumerate() {
            let ActionView::FunctionCall {
                method_name,
                args,
                deposit,
                gas: _,
            } = action
            else {
                continue;
            };

            let args_bytes: &[u8] = args.as_ref();
            let args_json = parse_args_json(args_bytes);
            let args_base64 = base64::prelude::BASE64_STANDARD.encode(args_bytes);

            if context.receiver_id == self.venear_contract_id
                && method_name == "on_lockup_update"
                && context.success
            {
                self.discover_lockup_from_on_lockup_update(
                    db,
                    state,
                    &context.predecessor_id,
                    &args_json,
                    context.block_height,
                    context.block_timestamp,
                )
                .await?;
            }

            if context.receiver_id == self.voting_contract_id
                && method_name == "on_get_snapshot"
                && context.success
            {
                self.insert_proposal_approval_finalized_event(
                    db,
                    context,
                    action_index as i32,
                    &args_json,
                )
                .await?;
            }

            if !state.known_lockups.contains(&context.receiver_id) {
                continue;
            }

            let staking_pool_account_id = args_json
                .as_ref()
                .and_then(|v| json_string(v, "staking_pool_account_id"));

            let owner_account_id = state.lockup_owners.get(&context.receiver_id).cloned();

            self.upsert_lockup_action(
                db,
                &context.receiver_id,
                owner_account_id.as_deref(),
                &context.predecessor_id,
                &context.receiver_id,
                method_name,
                if context.transaction_hash.is_some() {
                    "transaction"
                } else {
                    "receipt"
                },
                action_index as i32,
                args_json.clone(),
                args_base64,
                deposit.to_string(),
                context.transaction_hash.as_deref(),
                &context.receipt_id,
                &context.execution_outcome_id,
                context.success,
                context.block_height,
                context.block_timestamp,
                context.shard_id,
                staking_pool_account_id.as_deref(),
                None,
            )
            .await?;
        }

        Ok(())
    }

    async fn discover_lockup_from_on_lockup_update(
        &self,
        db: &impl ConnectionTrait,
        state: &mut ProcessState,
        predecessor_id: &str,
        args_json: &Option<JsonValue>,
        block_height: i64,
        block_timestamp: i64,
    ) -> Result<()> {
        let Some(args_json) = args_json else {
            return Ok(());
        };

        let Some(owner_account_id) = json_string(args_json, "owner_account_id") else {
            return Ok(());
        };

        let expected_lockup =
            derive_lockup_account_id(&owner_account_id, &self.lockup_account_suffix);

        if predecessor_id != expected_lockup {
            warn!(
                predecessor_id = predecessor_id,
                expected_lockup = expected_lockup,
                owner_account_id = owner_account_id,
                "on_lockup_update predecessor does not match derived lockup id"
            );
            return Ok(());
        }

        let lockup_version = json_u64(args_json, "version").map(|v| v as i64);

        self.upsert_lockup(
            db,
            &expected_lockup,
            &owner_account_id,
            lockup_version,
            "on_lockup_update",
            block_height,
            block_timestamp,
            true,
            None,
        )
        .await?;

        state.known_lockups.insert(expected_lockup.clone());
        state
            .lockup_owners
            .insert(expected_lockup, owner_account_id.clone());

        Ok(())
    }

    async fn insert_proposal_approval_finalized_event(
        &self,
        db: &impl ConnectionTrait,
        context: &ActionContext,
        action_index: i32,
        args_json: &Option<JsonValue>,
    ) -> Result<()> {
        let Some(args_json) = args_json else {
            return Ok(());
        };

        let Some(proposal_id_raw) = json_u64(args_json, "proposal_id") else {
            return Ok(());
        };

        let proposal_id =
            i64::try_from(proposal_id_raw).map_err(|_| anyhow!("proposal_id out of i64 range"))?;

        let payload = json!({
            "args": args_json,
            "source": "on_get_snapshot_callback",
        });

        self.upsert_proposal_event(
            db,
            proposal_id,
            "proposal_approve_finalized",
            Some(context.predecessor_id.clone()),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(payload),
            context.block_height,
            context.block_timestamp,
            context.shard_id,
            &context.receipt_id,
            &context.execution_outcome_id,
            &context.predecessor_id,
            &context.receiver_id,
            -1,
            action_index,
        )
        .await?;

        Ok(())
    }

    fn extract_actions_from_receipt(receipt: &ReceiptView) -> Option<Vec<ActionView>> {
        match &receipt.receipt {
            ReceiptEnumView::Action { actions, .. } => Some(actions.clone()),
            _ => None,
        }
    }

    async fn get_existing_balance_total_for_identity(
        &self,
        db: &impl ConnectionTrait,
        execution_outcome_id: &str,
        log_index: i32,
        data_index: i32,
        account_id: &str,
    ) -> Result<Option<u128>> {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT total_balance FROM near.near_venear_account_balances WHERE execution_outcome_id = $1 AND log_index = $2 AND data_index = $3 AND account_id = $4 LIMIT 1",
                vec![
                    execution_outcome_id.into(),
                    log_index.into(),
                    data_index.into(),
                    account_id.into(),
                ],
            ))
            .await?;

        let Some(row) = row else {
            return Ok(None);
        };

        let total_balance: String = row.try_get("", "total_balance")?;
        Ok(total_balance.parse::<u128>().ok())
    }

    async fn get_latest_total_balance(
        &self,
        db: &impl ConnectionTrait,
        state: &mut ProcessState,
        account_id: &str,
    ) -> Result<u128> {
        if let Some(value) = state.account_balances.get(account_id) {
            return Ok(*value);
        }

        let row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT total_balance FROM near.near_venear_account_balances WHERE account_id = $1 ORDER BY block_height DESC, id DESC LIMIT 1",
                vec![account_id.into()],
            ))
            .await?;

        let value = if let Some(row) = row {
            let total_balance: String = row.try_get("", "total_balance")?;
            total_balance.parse::<u128>().unwrap_or(0)
        } else {
            0
        };

        state.account_balances.insert(account_id.to_string(), value);
        Ok(value)
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_raw_event(
        &self,
        db: &impl ConnectionTrait,
        block_height: i64,
        block_timestamp: i64,
        shard_id: i32,
        receipt_id: &str,
        execution_outcome_id: &str,
        predecessor_account_id: &str,
        contract_id: &str,
        log_index: i32,
        data_index: i32,
        standard: Option<&str>,
        event_type: Option<&str>,
        success: bool,
        parse_error: Option<String>,
        event_data: Option<JsonValue>,
        log_text: &str,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_venear_events_raw (block_height, block_timestamp, shard_id, receipt_id, execution_outcome_id, predecessor_account_id, contract_id, log_index, data_index, standard, event_type, success, parse_error, event_data, log_text)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            ON CONFLICT (execution_outcome_id, log_index, data_index)
            DO UPDATE SET
              block_height = EXCLUDED.block_height,
              block_timestamp = EXCLUDED.block_timestamp,
              shard_id = EXCLUDED.shard_id,
              receipt_id = EXCLUDED.receipt_id,
              predecessor_account_id = EXCLUDED.predecessor_account_id,
              contract_id = EXCLUDED.contract_id,
              standard = EXCLUDED.standard,
              event_type = EXCLUDED.event_type,
              success = EXCLUDED.success,
              parse_error = EXCLUDED.parse_error,
              event_data = EXCLUDED.event_data,
              log_text = EXCLUDED.log_text",
            vec![
                block_height.into(),
                block_timestamp.into(),
                shard_id.into(),
                receipt_id.into(),
                execution_outcome_id.into(),
                predecessor_account_id.into(),
                contract_id.into(),
                log_index.into(),
                data_index.into(),
                standard.map(ToString::to_string).into(),
                event_type.map(ToString::to_string).into(),
                success.into(),
                parse_error.into(),
                event_data.into(),
                log_text.into(),
            ],
        ))
        .await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_lockup(
        &self,
        db: &impl ConnectionTrait,
        lockup_account_id: &str,
        owner_account_id: &str,
        lockup_version: Option<i64>,
        discovery_source: &str,
        block_height: i64,
        block_timestamp: i64,
        is_active: bool,
        deleted_at_height: Option<i64>,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_venear_lockups (lockup_account_id, owner_account_id, lockup_version, discovery_source, discovered_at_height, discovered_at_timestamp, last_seen_height, last_seen_timestamp, is_active, deleted_at_height, deleted_at_timestamp, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (lockup_account_id)
            DO UPDATE SET
              owner_account_id = EXCLUDED.owner_account_id,
              lockup_version = COALESCE(EXCLUDED.lockup_version, near.near_venear_lockups.lockup_version),
              discovery_source = EXCLUDED.discovery_source,
              discovered_at_height = LEAST(near.near_venear_lockups.discovered_at_height, EXCLUDED.discovered_at_height),
              discovered_at_timestamp = LEAST(near.near_venear_lockups.discovered_at_timestamp, EXCLUDED.discovered_at_timestamp),
              last_seen_height = GREATEST(near.near_venear_lockups.last_seen_height, EXCLUDED.last_seen_height),
              last_seen_timestamp = EXCLUDED.last_seen_timestamp,
              is_active = EXCLUDED.is_active,
              deleted_at_height = COALESCE(EXCLUDED.deleted_at_height, near.near_venear_lockups.deleted_at_height),
              deleted_at_timestamp = COALESCE(EXCLUDED.deleted_at_timestamp, near.near_venear_lockups.deleted_at_timestamp),
              metadata = COALESCE(EXCLUDED.metadata, near.near_venear_lockups.metadata)",
            vec![
                lockup_account_id.into(),
                owner_account_id.into(),
                lockup_version.into(),
                discovery_source.into(),
                block_height.into(),
                block_timestamp.into(),
                is_active.into(),
                deleted_at_height.into(),
                deleted_at_height.map(|_| block_timestamp).into(),
                Option::<JsonValue>::None.into(),
            ],
        ))
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_account_balance_row(
        &self,
        db: &impl ConnectionTrait,
        account_id: &str,
        event_type: &str,
        delta_amount: Option<String>,
        total_balance: u128,
        locked_near_balance: Option<u128>,
        lockup_update_nonce: Option<i64>,
        lockup_version: Option<i64>,
        block_height: i64,
        block_timestamp: i64,
        shard_id: i32,
        receipt_id: &str,
        execution_outcome_id: &str,
        predecessor_account_id: &str,
        contract_id: &str,
        log_index: i32,
        data_index: i32,
        payload: Option<JsonValue>,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_venear_account_balances (account_id, event_type, delta_amount, total_balance, locked_near_balance, lockup_update_nonce, lockup_version, block_height, block_timestamp, shard_id, receipt_id, execution_outcome_id, predecessor_account_id, contract_id, log_index, data_index, payload)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17)
            ON CONFLICT (execution_outcome_id, log_index, data_index, account_id)
            DO UPDATE SET
              event_type = EXCLUDED.event_type,
              delta_amount = EXCLUDED.delta_amount,
              total_balance = EXCLUDED.total_balance,
              locked_near_balance = EXCLUDED.locked_near_balance,
              lockup_update_nonce = EXCLUDED.lockup_update_nonce,
              lockup_version = EXCLUDED.lockup_version,
              block_height = EXCLUDED.block_height,
              block_timestamp = EXCLUDED.block_timestamp,
              shard_id = EXCLUDED.shard_id,
              receipt_id = EXCLUDED.receipt_id,
              predecessor_account_id = EXCLUDED.predecessor_account_id,
              contract_id = EXCLUDED.contract_id,
              payload = EXCLUDED.payload",
            vec![
                account_id.into(),
                event_type.into(),
                delta_amount.into(),
                total_balance.to_string().into(),
                locked_near_balance.map(|v| v.to_string()).into(),
                lockup_update_nonce.into(),
                lockup_version.into(),
                block_height.into(),
                block_timestamp.into(),
                shard_id.into(),
                receipt_id.into(),
                execution_outcome_id.into(),
                predecessor_account_id.into(),
                contract_id.into(),
                log_index.into(),
                data_index.into(),
                payload.into(),
            ],
        ))
        .await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_proposal_event(
        &self,
        db: &impl ConnectionTrait,
        proposal_id: i64,
        event_type: &str,
        account_id: Option<String>,
        proposer_id: Option<String>,
        reviewer_id: Option<String>,
        voting_start_time_sec: Option<i64>,
        snapshot_block_height: Option<i64>,
        snapshot_root: Option<String>,
        snapshot_total_venear: Option<String>,
        metadata: Option<JsonValue>,
        payload: Option<JsonValue>,
        block_height: i64,
        block_timestamp: i64,
        shard_id: i32,
        receipt_id: &str,
        execution_outcome_id: &str,
        predecessor_account_id: &str,
        contract_id: &str,
        log_index: i32,
        data_index: i32,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_venear_proposal_events (proposal_id, event_type, account_id, proposer_id, reviewer_id, voting_start_time_sec, snapshot_block_height, snapshot_root, snapshot_total_venear, metadata, payload, block_height, block_timestamp, shard_id, receipt_id, execution_outcome_id, predecessor_account_id, contract_id, log_index, data_index)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20)
            ON CONFLICT (execution_outcome_id, log_index, data_index, event_type)
            DO UPDATE SET
              proposal_id = EXCLUDED.proposal_id,
              account_id = EXCLUDED.account_id,
              proposer_id = EXCLUDED.proposer_id,
              reviewer_id = EXCLUDED.reviewer_id,
              voting_start_time_sec = EXCLUDED.voting_start_time_sec,
              snapshot_block_height = EXCLUDED.snapshot_block_height,
              snapshot_root = EXCLUDED.snapshot_root,
              snapshot_total_venear = EXCLUDED.snapshot_total_venear,
              metadata = EXCLUDED.metadata,
              payload = EXCLUDED.payload,
              block_height = EXCLUDED.block_height,
              block_timestamp = EXCLUDED.block_timestamp,
              shard_id = EXCLUDED.shard_id,
              receipt_id = EXCLUDED.receipt_id,
              predecessor_account_id = EXCLUDED.predecessor_account_id,
              contract_id = EXCLUDED.contract_id",
            vec![
                proposal_id.into(),
                event_type.into(),
                account_id.into(),
                proposer_id.into(),
                reviewer_id.into(),
                voting_start_time_sec.into(),
                snapshot_block_height.into(),
                snapshot_root.into(),
                snapshot_total_venear.into(),
                metadata.into(),
                payload.into(),
                block_height.into(),
                block_timestamp.into(),
                shard_id.into(),
                receipt_id.into(),
                execution_outcome_id.into(),
                predecessor_account_id.into(),
                contract_id.into(),
                log_index.into(),
                data_index.into(),
            ],
        ))
        .await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_vote_event(
        &self,
        db: &impl ConnectionTrait,
        proposal_id: i64,
        account_id: &str,
        vote: i32,
        account_balance: &str,
        event_type: &str,
        payload: Option<JsonValue>,
        block_height: i64,
        block_timestamp: i64,
        shard_id: i32,
        receipt_id: &str,
        execution_outcome_id: &str,
        predecessor_account_id: &str,
        contract_id: &str,
        log_index: i32,
        data_index: i32,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_venear_votes (proposal_id, account_id, vote, account_balance, event_type, payload, block_height, block_timestamp, shard_id, receipt_id, execution_outcome_id, predecessor_account_id, contract_id, log_index, data_index)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
            ON CONFLICT (execution_outcome_id, log_index, data_index, account_id, proposal_id)
            DO UPDATE SET
              vote = EXCLUDED.vote,
              account_balance = EXCLUDED.account_balance,
              event_type = EXCLUDED.event_type,
              payload = EXCLUDED.payload,
              block_height = EXCLUDED.block_height,
              block_timestamp = EXCLUDED.block_timestamp,
              shard_id = EXCLUDED.shard_id,
              receipt_id = EXCLUDED.receipt_id,
              predecessor_account_id = EXCLUDED.predecessor_account_id,
              contract_id = EXCLUDED.contract_id",
            vec![
                proposal_id.into(),
                account_id.into(),
                vote.into(),
                account_balance.into(),
                event_type.into(),
                payload.into(),
                block_height.into(),
                block_timestamp.into(),
                shard_id.into(),
                receipt_id.into(),
                execution_outcome_id.into(),
                predecessor_account_id.into(),
                contract_id.into(),
                log_index.into(),
                data_index.into(),
            ],
        ))
        .await?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn upsert_lockup_action(
        &self,
        db: &impl ConnectionTrait,
        lockup_account_id: &str,
        owner_account_id: Option<&str>,
        predecessor_account_id: &str,
        receiver_account_id: &str,
        method_name: &str,
        action_source: &str,
        action_index: i32,
        args_json: Option<JsonValue>,
        args_base64: String,
        attached_deposit: String,
        transaction_hash: Option<&str>,
        receipt_id: &str,
        execution_outcome_id: &str,
        success: bool,
        block_height: i64,
        block_timestamp: i64,
        shard_id: i32,
        staking_pool_account_id: Option<&str>,
        metadata: Option<JsonValue>,
    ) -> Result<()> {
        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_venear_lockup_actions (lockup_account_id, owner_account_id, predecessor_account_id, receiver_account_id, method_name, action_source, action_index, args_json, args_base64, attached_deposit, transaction_hash, receipt_id, execution_outcome_id, success, block_height, block_timestamp, shard_id, staking_pool_account_id, metadata)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19)
            ON CONFLICT (receipt_id, action_index, method_name, predecessor_account_id, receiver_account_id)
            DO UPDATE SET
              lockup_account_id = EXCLUDED.lockup_account_id,
              owner_account_id = EXCLUDED.owner_account_id,
              action_source = EXCLUDED.action_source,
              args_json = EXCLUDED.args_json,
              args_base64 = EXCLUDED.args_base64,
              attached_deposit = EXCLUDED.attached_deposit,
              transaction_hash = EXCLUDED.transaction_hash,
              execution_outcome_id = EXCLUDED.execution_outcome_id,
              success = EXCLUDED.success,
              block_height = EXCLUDED.block_height,
              block_timestamp = EXCLUDED.block_timestamp,
              shard_id = EXCLUDED.shard_id,
              staking_pool_account_id = EXCLUDED.staking_pool_account_id,
              metadata = EXCLUDED.metadata",
            vec![
                lockup_account_id.into(),
                owner_account_id.map(ToString::to_string).into(),
                predecessor_account_id.into(),
                receiver_account_id.into(),
                method_name.into(),
                action_source.into(),
                action_index.into(),
                args_json.into(),
                args_base64.into(),
                attached_deposit.into(),
                transaction_hash.map(ToString::to_string).into(),
                receipt_id.into(),
                execution_outcome_id.into(),
                success.into(),
                block_height.into(),
                block_timestamp.into(),
                shard_id.into(),
                staking_pool_account_id.map(ToString::to_string).into(),
                metadata.into(),
            ],
        ))
        .await?;

        Ok(())
    }
}

#[async_trait]
impl IndexerModule for VenearGovernanceModule {
    fn name(&self) -> &str {
        MODULE_NAME
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    async fn initialize(&self, db: &DatabaseConnection) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        self.load_known_lockups(db)
            .await
            .context("failed initializing lockup cache")?;

        Ok(())
    }

    #[instrument(skip(self, msg, db), fields(module = "venear_governance", height = %msg.block.header.height))]
    async fn process_block(&self, msg: &StreamerMessage, db: &DatabaseConnection) -> Result<()> {
        if !self.enabled {
            return Ok(());
        }

        let block_height = msg.block.header.height as i64;
        let block_timestamp = msg.block.header.timestamp as i64;
        let start = Instant::now();

        let txn = db.begin().await?;
        let mut state = self.build_process_state();

        for shard in &msg.shards {
            self.process_shard(shard, block_height, block_timestamp, &txn, &mut state)
                .await
                .with_context(|| {
                    format!(
                        "failed processing shard {} at block {}",
                        shard.shard_id, block_height
                    )
                })?;
        }

        txn.commit().await?;

        self.merge_process_state(&state);

        info!(
            block_height = block_height,
            elapsed_ms = start.elapsed().as_millis(),
            known_lockups = state.known_lockups.len(),
            "processed venear governance block"
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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use common::config::{
        AppConfig, DatabaseConfig, IndexerConfig, LoggingConfig, NearLakeConfig, NearModulesConfig,
        TelemetryConfig, VenearGovernanceModuleConfig,
    };
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    use test_support::{init_test_tracing, TestDatabase};

    fn build_test_config() -> AppConfig {
        let mut db = DatabaseConfig::default();
        db.url = "postgres://unused".to_string();

        let cfg = IndexerConfig {
            database: db,
            logging: LoggingConfig::default(),
            telemetry: TelemetryConfig::default(),
            discourse: None,
            near: Some(NearLakeConfig {
                chain: "mainnet".to_string(),
                start_height: 0,
                concurrency: Some(1),
                preload_pool_size: Some(1),
                modules: Some(NearModulesConfig {
                    raw: None,
                    accounts: None,
                    tokens: None,
                    staking_pools: None,
                    venear_governance: Some(VenearGovernanceModuleConfig {
                        enabled: true,
                        venear_contract_id: "venear.dao".to_string(),
                        voting_contract_id: "vote.dao".to_string(),
                        lockup_account_suffix: ".venear.dao".to_string(),
                        known_lockups: vec![],
                    }),
                }),
            }),
        };

        AppConfig::Indexer(Box::new(cfg))
    }

    async fn scalar_i64(db: &impl ConnectionTrait, sql: &str) -> Result<i64> {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                vec![],
            ))
            .await?
            .expect("expected one row");

        Ok(row.try_get("", "value")?)
    }

    async fn scalar_bool(db: &impl ConnectionTrait, sql: &str) -> Result<bool> {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                vec![],
            ))
            .await?
            .expect("expected one row");

        Ok(row.try_get("", "value")?)
    }

    async fn scalar_string(db: &impl ConnectionTrait, sql: &str) -> Result<String> {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                vec![],
            ))
            .await?
            .expect("expected one row");

        Ok(row.try_get("", "value")?)
    }

    async fn scalar_opt_string(db: &impl ConnectionTrait, sql: &str) -> Result<Option<String>> {
        let row = db
            .query_one(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                vec![],
            ))
            .await?
            .expect("expected one row");

        Ok(row.try_get("", "value")?)
    }

    #[tokio::test]
    async fn failed_outcome_event_is_written_to_raw_only() -> Result<()> {
        init_test_tracing();

        let db = TestDatabase::new().await;
        let conn = db.db();
        let module = VenearGovernanceModule::new(&build_test_config());
        module.initialize(conn.as_ref()).await?;
        let mut state = module.build_process_state();

        let logs = vec![r#"EVENT_JSON:{"standard":"venear","version":"1.0.0","event":"add_vote","data":[{"account_id":"alice.near","proposal_id":1,"vote":2,"account_balance":"10"}]}"#.to_string()];

        module
            .process_logs(
                &logs,
                "venear.dao",
                "alice.near",
                "rcpt-failed",
                "exec-failed",
                0,
                100,
                1000,
                false,
                conn.as_ref(),
                &mut state,
            )
            .await?;

        let raw_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw",
        )
        .await?;
        assert_eq!(raw_count, 1);

        let votes_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes",
        )
        .await?;
        assert_eq!(votes_count, 0);

        let raw_success = scalar_bool(
            conn.as_ref(),
            "SELECT success AS value FROM near.near_venear_events_raw WHERE execution_outcome_id = 'exec-failed' LIMIT 1",
        )
        .await?;
        assert!(!raw_success);

        let parse_error = scalar_opt_string(
            conn.as_ref(),
            "SELECT parse_error AS value FROM near.near_venear_events_raw WHERE execution_outcome_id = 'exec-failed' LIMIT 1",
        )
        .await?;
        assert_eq!(parse_error, None);

        Ok(())
    }

    #[tokio::test]
    async fn malformed_event_json_is_preserved_with_parse_error() -> Result<()> {
        init_test_tracing();

        let db = TestDatabase::new().await;
        let conn = db.db();
        let module = VenearGovernanceModule::new(&build_test_config());
        module.initialize(conn.as_ref()).await?;
        let mut state = module.build_process_state();

        let malformed_log = "EVENT_JSON:{\"standard\":\"venear\",\"event\":\"add_vote\",\"data\":[";

        module
            .process_logs(
                &[malformed_log.to_string()],
                "venear.dao",
                "alice.near",
                "rcpt-malformed",
                "exec-malformed",
                0,
                101,
                1001,
                true,
                conn.as_ref(),
                &mut state,
            )
            .await?;

        let raw_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_events_raw",
        )
        .await?;
        assert_eq!(raw_count, 1);

        let parse_error = scalar_string(
            conn.as_ref(),
            "SELECT parse_error AS value FROM near.near_venear_events_raw WHERE execution_outcome_id = 'exec-malformed' LIMIT 1",
        )
        .await?;
        assert!(parse_error.contains("failed to parse EVENT_JSON payload"));

        let stored_log = scalar_string(
            conn.as_ref(),
            "SELECT log_text AS value FROM near.near_venear_events_raw WHERE execution_outcome_id = 'exec-malformed' LIMIT 1",
        )
        .await?;
        assert_eq!(stored_log, malformed_log);

        let votes_count = scalar_i64(
            conn.as_ref(),
            "SELECT COUNT(*)::bigint AS value FROM near.near_venear_votes",
        )
        .await?;
        assert_eq!(votes_count, 0);

        Ok(())
    }
}
