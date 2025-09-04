use anyhow::Result;
use async_trait::async_trait;
use near_lake_framework::near_indexer_primitives::views::ExecutionOutcomeWithIdView;
use sea_orm::ActiveValue::NotSet;
use sea_orm::{DatabaseConnection, EntityTrait, Set, TransactionTrait};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, warn};

use crate::core::cursor;
use crate::IndexerModule;

const MODULE_NAME: &str = "tokens";

/// Tokens indexer module for NEP-141 and NEP-171 tokens
pub struct TokensModule {
    enabled: bool,
    track_nep141: bool,
    track_nep171: bool,
    contracts_filter: Vec<String>,
}

impl TokensModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        let (enabled, track_nep141, track_nep171, contracts_filter) = match config {
            common::config::AppConfig::Indexer(idx) => {
                let near_config = idx.near.as_ref();

                // Check if NEAR indexing is enabled at all
                let near_enabled = near_config.map(|c| c.chain != "disabled").unwrap_or(false);

                if !near_enabled {
                    return Self {
                        enabled: false,
                        track_nep141: false,
                        track_nep171: false,
                        contracts_filter: Vec::new(),
                    };
                }

                // Get tokens module configuration from config file
                let tokens_config = near_config
                    .and_then(|nc| nc.modules.as_ref())
                    .and_then(|mc| mc.tokens.as_ref());

                match tokens_config {
                    Some(tc) => (
                        tc.enabled,
                        tc.track_nep141,
                        tc.track_nep171,
                        tc.contracts.clone(),
                    ),
                    None => {
                        // Default configuration when not specified
                        (false, true, true, Vec::new())
                    }
                }
            }
            _ => (false, false, false, Vec::new()),
        };

        Self {
            enabled,
            track_nep141,
            track_nep171,
            contracts_filter,
        }
    }

    /// Parse NEAR event logs from execution outcomes
    fn parse_event_logs(&self, outcome: &ExecutionOutcomeWithIdView) -> Result<Vec<TokenEvent>> {
        let mut events = Vec::new();

        for log in &outcome.outcome.logs {
            if !log.starts_with("EVENT_JSON:") {
                continue;
            }

            // Remove EVENT_JSON: prefix
            let json_str = &log[11..];

            match serde_json::from_str::<Value>(json_str) {
                Ok(event_value) => {
                    if let Some(event) = self.parse_token_event(&event_value, outcome) {
                        events.push(event);
                    }
                }
                Err(e) => {
                    warn!(
                        receipt_id = %outcome.id,
                        log = %log,
                        error = %e,
                        "Failed to parse event log JSON"
                    );
                }
            }
        }

        Ok(events)
    }

    /// Parse individual token event from JSON value
    fn parse_token_event(
        &self,
        event_value: &Value,
        outcome: &ExecutionOutcomeWithIdView,
    ) -> Option<TokenEvent> {
        let standard = event_value.get("standard")?.as_str()?;
        let version = event_value.get("version")?.as_str()?;
        let event_type = event_value.get("event")?.as_str()?;
        let data = event_value.get("data")?.as_array()?;

        match standard {
            "nep141" if self.track_nep141 => {
                self.parse_nep141_event(event_type, data, outcome, version)
            }
            "nep171" if self.track_nep171 => {
                self.parse_nep171_event(event_type, data, outcome, version)
            }
            _ => None,
        }
    }

    /// Parse NEP-141 fungible token events
    fn parse_nep141_event(
        &self,
        event_type: &str,
        data: &[Value],
        outcome: &ExecutionOutcomeWithIdView,
        _version: &str,
    ) -> Option<TokenEvent> {
        let contract_id = outcome.outcome.executor_id.to_string();

        // Skip if contract filter is set and doesn't include this contract
        if !self.contracts_filter.is_empty() && !self.contracts_filter.contains(&contract_id) {
            return None;
        }

        match event_type {
            "ft_mint" => {
                if let Some(item) = data.iter().next() {
                    let owner_id = item.get("owner_id")?.as_str()?.to_string();
                    let amount = item.get("amount")?.as_str()?;
                    let memo = item
                        .get("memo")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    return Some(TokenEvent {
                        contract_id: contract_id.clone(),
                        token_standard: "nep141".to_string(),
                        event_type: "ft_mint".to_string(),
                        from_account_id: None,
                        to_account_id: Some(owner_id),
                        amount: Some(amount.to_string()),
                        token_ids: None,
                        memo,
                        authorized_id: None,
                    });
                }
            }
            "ft_transfer" => {
                if let Some(item) = data.iter().next() {
                    let old_owner_id = item.get("old_owner_id")?.as_str()?.to_string();
                    let new_owner_id = item.get("new_owner_id")?.as_str()?.to_string();
                    let amount = item.get("amount")?.as_str()?;
                    let memo = item
                        .get("memo")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    return Some(TokenEvent {
                        contract_id: contract_id.clone(),
                        token_standard: "nep141".to_string(),
                        event_type: "ft_transfer".to_string(),
                        from_account_id: Some(old_owner_id),
                        to_account_id: Some(new_owner_id),
                        amount: Some(amount.to_string()),
                        token_ids: None,
                        memo,
                        authorized_id: None,
                    });
                }
            }
            "ft_burn" => {
                if let Some(item) = data.iter().next() {
                    let owner_id = item.get("owner_id")?.as_str()?.to_string();
                    let amount = item.get("amount")?.as_str()?;
                    let memo = item
                        .get("memo")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    return Some(TokenEvent {
                        contract_id: contract_id.clone(),
                        token_standard: "nep141".to_string(),
                        event_type: "ft_burn".to_string(),
                        from_account_id: Some(owner_id),
                        to_account_id: None,
                        amount: Some(amount.to_string()),
                        token_ids: None,
                        memo,
                        authorized_id: None,
                    });
                }
            }
            _ => return None,
        }

        None
    }

    /// Parse NEP-171 NFT events
    fn parse_nep171_event(
        &self,
        event_type: &str,
        data: &[Value],
        outcome: &ExecutionOutcomeWithIdView,
        _version: &str,
    ) -> Option<TokenEvent> {
        let contract_id = outcome.outcome.executor_id.to_string();

        // Skip if contract filter is set and doesn't include this contract
        if !self.contracts_filter.is_empty() && !self.contracts_filter.contains(&contract_id) {
            return None;
        }

        match event_type {
            "nft_mint" => {
                if let Some(item) = data.iter().next() {
                    let owner_id = item.get("owner_id")?.as_str()?.to_string();
                    let token_ids = item
                        .get("token_ids")?
                        .as_array()?
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>();
                    let memo = item
                        .get("memo")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    return Some(TokenEvent {
                        contract_id: contract_id.clone(),
                        token_standard: "nep171".to_string(),
                        event_type: "nft_mint".to_string(),
                        from_account_id: None,
                        to_account_id: Some(owner_id),
                        amount: None,
                        token_ids: Some(serde_json::to_string(&token_ids).unwrap()),
                        memo,
                        authorized_id: None,
                    });
                }
            }
            "nft_transfer" => {
                if let Some(item) = data.iter().next() {
                    let old_owner_id = item.get("old_owner_id")?.as_str()?.to_string();
                    let new_owner_id = item.get("new_owner_id")?.as_str()?.to_string();
                    let token_ids = item
                        .get("token_ids")?
                        .as_array()?
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>();
                    let memo = item
                        .get("memo")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let authorized_id = item
                        .get("authorized_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    return Some(TokenEvent {
                        contract_id: contract_id.clone(),
                        token_standard: "nep171".to_string(),
                        event_type: "nft_transfer".to_string(),
                        from_account_id: Some(old_owner_id),
                        to_account_id: Some(new_owner_id),
                        amount: None,
                        token_ids: Some(serde_json::to_string(&token_ids).unwrap()),
                        memo,
                        authorized_id,
                    });
                }
            }
            "nft_burn" => {
                if let Some(item) = data.iter().next() {
                    let owner_id = item.get("owner_id")?.as_str()?.to_string();
                    let token_ids = item
                        .get("token_ids")?
                        .as_array()?
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>();
                    let memo = item
                        .get("memo")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    let authorized_id = item
                        .get("authorized_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    return Some(TokenEvent {
                        contract_id: contract_id.clone(),
                        token_standard: "nep171".to_string(),
                        event_type: "nft_burn".to_string(),
                        from_account_id: Some(owner_id),
                        to_account_id: None,
                        amount: None,
                        token_ids: Some(serde_json::to_string(&token_ids).unwrap()),
                        memo,
                        authorized_id,
                    });
                }
            }
            _ => return None,
        }

        None
    }

    /// Process and store token events
    async fn process_token_events(
        &self,
        events: Vec<TokenEvent>,
        block_height: i64,
        block_time: i64,
        tx_hash: &str,
        receipt_hash: &str,
        db: &DatabaseConnection,
    ) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let txn = db.begin().await?;

        // Track unique contracts seen in this batch
        let mut contracts_seen: HashMap<String, String> = HashMap::new();

        for (log_index, event) in events.iter().enumerate() {
            // Track contract if not seen before
            if !contracts_seen.contains_key(&event.contract_id) {
                contracts_seen.insert(event.contract_id.clone(), event.token_standard.clone());
            }

            // Insert transfer event
            debug!(
                contract_id = %event.contract_id,
                event_type = %event.event_type,
                standard = %event.token_standard,
                "Processing token event"
            );

            // Parse amount to Decimal if present
            let amount_decimal = event
                .amount
                .as_ref()
                .and_then(|a| rust_decimal::Decimal::from_str_exact(a).ok());

            let transfer = entities::near_token_transfers::ActiveModel {
                id: NotSet,
                contract_id: Set(event.contract_id.clone()),
                token_standard: Set(event.token_standard.clone()),
                event_type: Set(event.event_type.clone()),
                from_account_id: Set(event.from_account_id.clone()),
                to_account_id: Set(event.to_account_id.clone()),
                amount: Set(amount_decimal),
                token_ids: Set(event.token_ids.clone()),
                memo: Set(event.memo.clone()),
                authorized_id: Set(event.authorized_id.clone()),
                block_height: Set(block_height),
                block_time: Set(block_time),
                tx_hash: Set(tx_hash.to_string()),
                receipt_hash: Set(receipt_hash.to_string()),
                log_index: Set(log_index as i32),
            };

            // Prevent duplicate inserts on replay by ignoring conflicts against
            // the unique index (receipt_hash, log_index, event_type).
            entities::near_token_transfers::Entity::insert(transfer)
                .on_conflict_do_nothing()
                .exec(&txn)
                .await?;
        }

        // Update contract registry for new contracts
        for (contract_id, token_standard) in contracts_seen {
            debug!(
                contract_id = %contract_id,
                standard = %token_standard,
                "Updating contract registry"
            );

            let token_contract = entities::near_tokens::ActiveModel {
                contract_id: Set(contract_id.clone()),
                token_standard: Set(token_standard.clone()),
                name: Set(None),
                symbol: Set(None),
                decimals: Set(None),
                total_supply: Set(None),
                icon: Set(None),
                reference: Set(None),
                reference_hash: Set(None),
                base_uri: Set(None),
                first_seen_height: Set(Some(block_height)),
                first_seen_time: Set(Some(block_time)),
                last_seen_height: Set(Some(block_height)),
                last_seen_time: Set(Some(block_time)),
            };

            // Upsert contract - insert if not exists, update last_seen if exists
            use entities::near_tokens::Column as TC;
            use sea_orm::sea_query::OnConflict;

            entities::near_tokens::Entity::insert(token_contract)
                .on_conflict(
                    OnConflict::column(TC::ContractId)
                        .update_columns([TC::LastSeenHeight, TC::LastSeenTime])
                        .to_owned(),
                )
                .exec(&txn)
                .await?;
        }

        txn.commit().await?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct TokenEvent {
    contract_id: String,
    token_standard: String,
    event_type: String,
    from_account_id: Option<String>,
    to_account_id: Option<String>,
    amount: Option<String>,
    token_ids: Option<String>, // JSON array for NFTs
    memo: Option<String>,
    authorized_id: Option<String>,
}

#[async_trait]
impl IndexerModule for TokensModule {
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
        // Always skip if this block is at or before the persisted cursor height
        if let Some(prev) = self.get_cursor(db).await? {
            if height <= prev {
                debug!(
                    height,
                    module = MODULE_NAME,
                    "skipping block at/before cursor"
                );
                return Ok(());
            }
        }
        let ts = msg.block.header.timestamp_nanosec as i64;

        // Create a map of transaction hashes from chunks
        let mut tx_hash_map = std::collections::HashMap::new();
        for shard in &msg.shards {
            if let Some(chunk) = &shard.chunk {
                for tx in &chunk.transactions {
                    let tx_hash = tx.transaction.hash.to_string();
                    for receipt in &tx.outcome.execution_outcome.outcome.receipt_ids {
                        tx_hash_map.insert(receipt.to_string(), tx_hash.clone());
                    }
                }
            }
        }

        // Process receipt execution outcomes
        for shard in &msg.shards {
            // Process transactions directly from chunks (for direct token events)
            if let Some(chunk) = &shard.chunk {
                for tx in &chunk.transactions {
                    let tx_hash = tx.transaction.hash.to_string();
                    let events = self.parse_event_logs(&tx.outcome.execution_outcome)?;
                    if !events.is_empty() {
                        // For transaction logs, use transaction hash as receipt hash too
                        self.process_token_events(events, height, ts, &tx_hash, &tx_hash, db)
                            .await?;
                    }
                }
            }

            // Process receipt execution outcomes
            for reo in &shard.receipt_execution_outcomes {
                let receipt_hash = reo.receipt.receipt_id.to_string();
                let tx_hash = tx_hash_map.get(&receipt_hash).cloned().unwrap_or_else(|| {
                    // Fallback: try to extract from receipt's predecessor or use receipt ID
                    receipt_hash.clone()
                });

                let events = self.parse_event_logs(&reo.execution_outcome)?;
                if !events.is_empty() {
                    self.process_token_events(events, height, ts, &tx_hash, &receipt_hash, db)
                        .await?;
                }
            }
        }

        debug!(height, module = MODULE_NAME, "processed block");
        Ok(())
    }

    async fn get_cursor(&self, db: &DatabaseConnection) -> Result<Option<i64>> {
        cursor::get_module_cursor(db, MODULE_NAME).await
    }

    async fn update_cursor(&self, db: &DatabaseConnection, height: i64) -> Result<()> {
        // Only advance cursor when moving forward
        let current = self.get_cursor(db).await?;
        if current.map(|h| height > h).unwrap_or(true) {
            cursor::upsert_module_cursor(db, MODULE_NAME, height).await?;
        }
        Ok(())
    }
}
