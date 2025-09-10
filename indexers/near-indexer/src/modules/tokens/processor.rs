use anyhow::{Context, Result};
use async_trait::async_trait;
use entities::*;
use near_indexer_primitives::{IndexerShard, StreamerMessage};
use sea_orm::{
    ActiveModelTrait, ConnectionTrait, DatabaseConnection, EntityTrait, Set, TransactionTrait,
};
use serde_json;
use tracing::{debug, info};

use crate::core::cursor;
use crate::modules::tokens::{events, state, types};
use crate::IndexerModule;

const MODULE_NAME: &str = "tokens";

pub struct TokensModule {
    enabled: bool,
    track_nep141: bool,
    track_nep171: bool,
    track_nep245: bool,
    contract_whitelist: Option<Vec<String>>,
}

impl TokensModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        let (enabled, track_nep141, track_nep171, track_nep245, contract_whitelist) = match config {
            common::config::AppConfig::Indexer(idx) => {
                let near_config = idx.near.as_ref();

                // Check if NEAR indexing is enabled at all
                let near_enabled = near_config.map(|c| c.chain != "disabled").unwrap_or(false);

                if !near_enabled {
                    return Self {
                        enabled: false,
                        track_nep141: false,
                        track_nep171: false,
                        track_nep245: false,
                        contract_whitelist: None,
                    };
                }

                // Get tokens module configuration
                let tokens_config = near_config
                    .and_then(|c| c.modules.as_ref())
                    .and_then(|m| m.tokens.as_ref());

                let enabled = tokens_config.map(|t| t.enabled).unwrap_or(false);
                let track_nep141 = tokens_config.map(|t| t.track_nep141).unwrap_or(true);
                let track_nep171 = tokens_config.map(|t| t.track_nep171).unwrap_or(true);
                let track_nep245 = tokens_config.map(|t| t.track_nep245).unwrap_or(true);
                let contract_whitelist = tokens_config.and_then(|t| {
                    if t.contracts.is_empty() {
                        None
                    } else {
                        Some(t.contracts.clone())
                    }
                });

                (
                    enabled,
                    track_nep141,
                    track_nep171,
                    track_nep245,
                    contract_whitelist,
                )
            }
            _ => (false, false, false, false, None),
        };

        Self {
            enabled,
            track_nep141,
            track_nep171,
            track_nep245,
            contract_whitelist,
        }
    }

    fn should_track_standard(&self, standard: &types::TokenStandard) -> bool {
        match standard {
            types::TokenStandard::Nep141 => self.track_nep141,
            types::TokenStandard::Nep171 => self.track_nep171,
            types::TokenStandard::Nep245 => self.track_nep245,
        }
    }

    fn should_track_contract(&self, contract_id: &str) -> bool {
        match &self.contract_whitelist {
            Some(whitelist) => whitelist.contains(&contract_id.to_string()),
            None => true, // Track all contracts if no whitelist
        }
    }

    async fn process_execution_outcome_logs(
        &self,
        shard: &IndexerShard,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // Process transaction execution outcomes
        if let Some(chunk) = &shard.chunk {
            for tx_with_outcome in &chunk.transactions {
                let outcome = &tx_with_outcome.outcome.execution_outcome;
                let transaction = &tx_with_outcome.transaction;

                self.process_logs(
                    &outcome.outcome.logs,
                    transaction.receiver_id.as_ref(),
                    &outcome.id.to_string(),
                    transaction.signer_id.as_ref(),
                    shard.shard_id.into(),
                    block_height,
                    block_timestamp,
                    db,
                )
                .await?;
            }
        }

        // Process receipt execution outcomes
        for outcome_with_receipt in &shard.receipt_execution_outcomes {
            let outcome = &outcome_with_receipt.execution_outcome;
            let receipt = &outcome_with_receipt.receipt;

            self.process_logs(
                &outcome.outcome.logs,
                receipt.receiver_id.as_ref(),
                &outcome.id.to_string(),
                receipt.predecessor_id.as_ref(),
                shard.shard_id.into(),
                block_height,
                block_timestamp,
                db,
            )
            .await?;
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn process_logs(
        &self,
        logs: &[String],
        contract_id: &str,
        execution_outcome_id: &str,
        predecessor_id: &str,
        shard_id: u64,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // Check if we should track this contract
        if !self.should_track_contract(contract_id) {
            return Ok(());
        }

        // Extract token events from logs
        let token_events = events::extract_token_events_from_logs(logs);

        if token_events.is_empty() {
            return Ok(());
        }

        debug!(
            "Processing {} token events from contract {} at block {}",
            token_events.len(),
            contract_id,
            block_height
        );

        for (log_index, parsed_event) in token_events {
            // Check if we should track this standard
            if !self.should_track_standard(&parsed_event.standard) {
                debug!(
                    "Skipping {} event (not tracked)",
                    parsed_event.standard.as_str()
                );
                continue;
            }

            // Store raw event log
            let raw_event_id = self
                .store_raw_event_log(
                    logs.get(log_index).unwrap(),
                    execution_outcome_id,
                    predecessor_id,
                    contract_id,
                    log_index as i32,
                    shard_id as i32,
                    block_height,
                    block_timestamp,
                    db,
                )
                .await?;

            // Store parsed event
            let event_id = self
                .store_parsed_event(
                    raw_event_id,
                    &parsed_event,
                    contract_id,
                    block_height,
                    block_timestamp,
                    db,
                )
                .await?;

            // Update contract registry
            self.update_contract_registry(contract_id, &parsed_event.standard, block_height, db)
                .await?;

            // Update token balances
            state::TokenStateManager::update_balances_from_event(
                &parsed_event,
                contract_id,
                block_height,
                db,
            )
            .await?;

            // Store transfer record if it's a transfer event
            if parsed_event.event_type.is_transfer() {
                self.store_transfer_record(
                    event_id,
                    &parsed_event,
                    contract_id,
                    block_height,
                    block_timestamp,
                    db,
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn store_raw_event_log(
        &self,
        log_text: &str,
        execution_outcome_id: &str,
        predecessor_id: &str,
        contract_id: &str,
        log_index: i32,
        shard_id: i32,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<i64> {
        let raw_event = near_token_events_raw::ActiveModel {
            id: sea_orm::NotSet,
            block_height: Set(block_height),
            block_timestamp: Set(block_timestamp),
            shard_id: Set(shard_id),
            receipt_id: Set("".to_string()), // We don't have receipt_id from this context
            execution_outcome_id: Set(execution_outcome_id.to_string()),
            predecessor_account_id: Set(predecessor_id.to_string()),
            contract_id: Set(contract_id.to_string()),
            log_index: Set(log_index),
            log_text: Set(log_text.to_string()),
            created_at: Set(Some(chrono::Utc::now())),
        };

        let result = raw_event.insert(db).await?;
        Ok(result.id)
    }

    async fn store_parsed_event(
        &self,
        raw_event_id: i64,
        parsed_event: &types::ParsedTokenEvent,
        contract_id: &str,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<i64> {
        let affected_accounts = parsed_event.affected_accounts.join(",");

        let event = near_token_events::ActiveModel {
            id: sea_orm::NotSet,
            raw_event_id: Set(raw_event_id),
            block_height: Set(block_height),
            block_timestamp: Set(block_timestamp),
            contract_id: Set(contract_id.to_string()),
            standard: Set(parsed_event.standard.as_str().to_string()),
            version: Set("1.0.0".to_string()), // Default version
            event_type: Set(parsed_event.event_type.as_str().to_string()),
            event_data: Set(parsed_event.data.clone()),
            affected_accounts: Set(affected_accounts),
        };

        let result = event.insert(db).await?;
        Ok(result.id)
    }

    async fn update_contract_registry(
        &self,
        contract_id: &str,
        standard: &types::TokenStandard,
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // Check if contract exists
        let existing = near_token_contracts::Entity::find_by_id(contract_id)
            .one(db)
            .await?;

        if let Some(contract) = existing {
            // Update existing contract
            let mut standards: Vec<String> =
                serde_json::from_value(contract.standards_detected).unwrap_or_default();

            let standard_str = standard.as_str().to_string();
            if !standards.contains(&standard_str) {
                standards.push(standard_str);
            }

            let updated_contract = near_token_contracts::ActiveModel {
                contract_id: Set(contract_id.to_string()),
                standards_detected: Set(serde_json::to_value(standards)?),
                last_activity_block: Set(block_height),
                total_events: Set(contract.total_events + 1),
                ..Default::default()
            };

            updated_contract.update(db).await?;
        } else {
            // Create new contract
            let new_contract = near_token_contracts::ActiveModel {
                contract_id: Set(contract_id.to_string()),
                standards_detected: Set(serde_json::to_value(vec![standard.as_str()])?),
                ft_metadata: Set(None),
                nft_metadata: Set(None),
                mt_metadata: Set(None),
                first_seen_block: Set(block_height),
                last_activity_block: Set(block_height),
                total_events: Set(1),
            };

            new_contract.insert(db).await?;
        }

        Ok(())
    }

    async fn store_transfer_record(
        &self,
        event_id: i64,
        parsed_event: &types::ParsedTokenEvent,
        contract_id: &str,
        block_height: i64,
        block_timestamp: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let data = &parsed_event.data;

        let from_account = data.get("old_owner_id").and_then(|v| v.as_str());
        let to_account = data.get("new_owner_id").and_then(|v| v.as_str());
        let memo = data.get("memo").and_then(|v| v.as_str());

        match parsed_event.standard {
            types::TokenStandard::Nep141 => {
                let amount = data.get("amount").and_then(|v| v.as_str()).unwrap_or("0");

                let transfer = near_token_transfers::ActiveModel {
                    id: sea_orm::NotSet,
                    event_id: Set(event_id),
                    block_height: Set(block_height),
                    block_timestamp: Set(block_timestamp),
                    contract_id: Set(contract_id.to_string()),
                    standard: Set(parsed_event.standard.as_str().to_string()),
                    token_type_id: Set(None),
                    token_id: Set(None),
                    from_account: Set(from_account.map(|s| s.to_string())),
                    to_account: Set(to_account.map(|s| s.to_string())),
                    amount: Set(amount.to_string()),
                    memo: Set(memo.map(|s| s.to_string())),
                };

                transfer.insert(db).await?;
            }
            types::TokenStandard::Nep171 => {
                let empty_vec = vec![];
                let token_ids = data
                    .get("token_ids")
                    .and_then(|v| v.as_array())
                    .unwrap_or(&empty_vec);

                for token_id_value in token_ids {
                    if let Some(token_id) = token_id_value.as_str() {
                        let transfer = near_token_transfers::ActiveModel {
                            id: sea_orm::NotSet,
                            event_id: Set(event_id),
                            block_height: Set(block_height),
                            block_timestamp: Set(block_timestamp),
                            contract_id: Set(contract_id.to_string()),
                            standard: Set(parsed_event.standard.as_str().to_string()),
                            token_type_id: Set(None),
                            token_id: Set(Some(token_id.to_string())),
                            from_account: Set(from_account.map(|s| s.to_string())),
                            to_account: Set(to_account.map(|s| s.to_string())),
                            amount: Set("1".to_string()),
                            memo: Set(memo.map(|s| s.to_string())),
                        };

                        transfer.insert(db).await?;
                    }
                }
            }
            types::TokenStandard::Nep245 => {
                let empty_token_vec = vec![];
                let empty_amount_vec = vec![];
                let token_ids = data
                    .get("token_ids")
                    .and_then(|v| v.as_array())
                    .unwrap_or(&empty_token_vec);
                let amounts = data
                    .get("amounts")
                    .and_then(|v| v.as_array())
                    .unwrap_or(&empty_amount_vec);

                for (token_id_value, amount_value) in token_ids.iter().zip(amounts.iter()) {
                    if let (Some(token_id), Some(amount)) =
                        (token_id_value.as_str(), amount_value.as_str())
                    {
                        // Parse token_type_id from token_id if it contains ":"
                        let (token_type_id, final_token_id) = if token_id.contains(':') {
                            let parts: Vec<&str> = token_id.splitn(2, ':').collect();
                            (Some(parts[0].to_string()), Some(parts[1].to_string()))
                        } else {
                            (None, Some(token_id.to_string()))
                        };

                        let transfer = near_token_transfers::ActiveModel {
                            id: sea_orm::NotSet,
                            event_id: Set(event_id),
                            block_height: Set(block_height),
                            block_timestamp: Set(block_timestamp),
                            contract_id: Set(contract_id.to_string()),
                            standard: Set(parsed_event.standard.as_str().to_string()),
                            token_type_id: Set(token_type_id),
                            token_id: Set(final_token_id),
                            from_account: Set(from_account.map(|s| s.to_string())),
                            to_account: Set(to_account.map(|s| s.to_string())),
                            amount: Set(amount.to_string()),
                            memo: Set(memo.map(|s| s.to_string())),
                        };

                        transfer.insert(db).await?;
                    }
                }
            }
        }

        Ok(())
    }
}

#[async_trait]
impl IndexerModule for TokensModule {
    fn name(&self) -> &str {
        MODULE_NAME
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    async fn process_block(&self, msg: &StreamerMessage, db: &DatabaseConnection) -> Result<()> {
        let block_height = msg.block.header.height as i64;
        let block_timestamp = msg.block.header.timestamp as i64;

        debug!("Processing token events for block {}", block_height);

        // Use transaction for consistency
        let txn = db.begin().await?;

        for shard in &msg.shards {
            self.process_execution_outcome_logs(shard, block_height, block_timestamp, &txn)
                .await
                .with_context(|| {
                    format!(
                        "Failed to process shard {} at block {}",
                        shard.shard_id, block_height
                    )
                })?;
        }

        txn.commit().await?;

        info!("Processed token events for block {}", block_height);
        Ok(())
    }

    async fn get_cursor(&self, db: &DatabaseConnection) -> Result<Option<i64>> {
        cursor::get_module_cursor(db, MODULE_NAME).await
    }

    async fn update_cursor(&self, db: &DatabaseConnection, height: i64) -> Result<()> {
        cursor::upsert_module_cursor(db, MODULE_NAME, height).await
    }
}
