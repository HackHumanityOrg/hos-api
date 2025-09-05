use anyhow::Result;
use async_trait::async_trait;
use sea_orm::ActiveValue::NotSet;
use sea_orm::{DatabaseConnection, EntityTrait, Set, TransactionTrait};
use serde_json::Value;
// Prefer standard event parsing via near-lake-primitives where possible
use crate::core::events as lake_events;
use std::collections::HashMap;
use tracing::debug;

use crate::core::cursor;
use crate::IndexerModule;

const MODULE_NAME: &str = "tokens";

/// Tokens indexer module for NEP-141 and NEP-171 tokens
pub struct TokensModule {
    enabled: bool,
    track_nep141: bool,
    track_nep171: bool,
    track_nep245: bool,
    contracts_filter: Vec<String>,
}

impl TokensModule {
    pub fn new(config: &common::config::AppConfig) -> Self {
        let (enabled, track_nep141, track_nep171, track_nep245, contracts_filter) = match config {
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
                        tc.track_nep245,
                        tc.contracts.clone(),
                    ),
                    None => {
                        // Default configuration when not specified
                        (false, true, true, true, Vec::new())
                    }
                }
            }
            _ => (false, false, false, false, Vec::new()),
        };

        Self {
            enabled,
            track_nep141,
            track_nep171,
            track_nep245,
            contracts_filter,
        }
    }

    // Note: event parsing is centralized at block-level via collect_block_events

    /// Parse individual token event from JSON value with explicit executor id
    fn parse_token_event_with_executor(
        &self,
        event_value: &Value,
        executor_id: &str,
    ) -> Option<TokenEvent> {
        let standard = event_value.get("standard")?.as_str()?;
        let version = event_value.get("version")?.as_str()?;
        let event_type = event_value.get("event")?.as_str()?;
        let data = event_value.get("data")?.as_array()?;

        match standard {
            "nep141" if self.track_nep141 => {
                self.parse_nep141_event(event_type, data, executor_id, version)
            }
            "nep171" if self.track_nep171 => {
                self.parse_nep171_event(event_type, data, executor_id, version)
            }
            "nep245" if self.track_nep245 => {
                self.parse_nep245_event(event_type, data, executor_id, version)
            }
            _ => None,
        }
    }

    /// Parse NEP-141 fungible token events
    fn parse_nep141_event(
        &self,
        event_type: &str,
        data: &[Value],
        executor_id: &str,
        _version: &str,
    ) -> Option<TokenEvent> {
        let contract_id = executor_id.to_string();

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
                        amounts: None,
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
                        amounts: None,
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
                        amounts: None,
                        memo,
                        authorized_id: None,
                    });
                }
            }
            _ => return None,
        }

        None
    }

    /// Parse NEP-245 MultiToken events (batch token_ids + amounts arrays)
    fn parse_nep245_event(
        &self,
        event_type: &str,
        data: &[Value],
        executor_id: &str,
        _version: &str,
    ) -> Option<TokenEvent> {
        let contract_id = executor_id.to_string();

        if !self.contracts_filter.is_empty() && !self.contracts_filter.contains(&contract_id) {
            return None;
        }

        // We compress arrays to JSON strings in TokenEvent
        match event_type {
            "mt_mint" => {
                if let Some(item) = data.iter().next() {
                    let owner_id = item.get("owner_id")?.as_str()?.to_string();
                    let token_ids = item.get("token_ids")?.as_array()?;
                    let amounts = item.get("amounts")?.as_array()?;
                    if token_ids.len() != amounts.len() {
                        return None;
                    }
                    let token_ids = token_ids
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>();
                    let amounts = amounts
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>();
                    let memo = item
                        .get("memo")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    return Some(TokenEvent {
                        contract_id: contract_id.clone(),
                        token_standard: "nep245".to_string(),
                        event_type: "mt_mint".to_string(),
                        from_account_id: None,
                        to_account_id: Some(owner_id),
                        amount: None,
                        token_ids: Some(serde_json::to_string(&token_ids).unwrap()),
                        amounts: Some(serde_json::to_string(&amounts).unwrap()),
                        memo,
                        authorized_id: None,
                    });
                }
            }
            "mt_transfer" => {
                if let Some(item) = data.iter().next() {
                    let old_owner_id = item.get("old_owner_id")?.as_str()?.to_string();
                    let new_owner_id = item.get("new_owner_id")?.as_str()?.to_string();
                    let token_ids = item.get("token_ids")?.as_array()?;
                    let amounts = item.get("amounts")?.as_array()?;
                    if token_ids.len() != amounts.len() {
                        return None;
                    }
                    let token_ids = token_ids
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>();
                    let amounts = amounts
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
                        token_standard: "nep245".to_string(),
                        event_type: "mt_transfer".to_string(),
                        from_account_id: Some(old_owner_id),
                        to_account_id: Some(new_owner_id),
                        amount: None,
                        token_ids: Some(serde_json::to_string(&token_ids).unwrap()),
                        amounts: Some(serde_json::to_string(&amounts).unwrap()),
                        memo,
                        authorized_id,
                    });
                }
            }
            "mt_burn" => {
                if let Some(item) = data.iter().next() {
                    let owner_id = item.get("owner_id")?.as_str()?.to_string();
                    let token_ids = item.get("token_ids")?.as_array()?;
                    let amounts = item.get("amounts")?.as_array()?;
                    if token_ids.len() != amounts.len() {
                        return None;
                    }
                    let token_ids = token_ids
                        .iter()
                        .filter_map(|v| v.as_str())
                        .collect::<Vec<_>>();
                    let amounts = amounts
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
                        token_standard: "nep245".to_string(),
                        event_type: "mt_burn".to_string(),
                        from_account_id: Some(owner_id),
                        to_account_id: None,
                        amount: None,
                        token_ids: Some(serde_json::to_string(&token_ids).unwrap()),
                        amounts: Some(serde_json::to_string(&amounts).unwrap()),
                        memo,
                        authorized_id,
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
        executor_id: &str,
        _version: &str,
    ) -> Option<TokenEvent> {
        let contract_id = executor_id.to_string();

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
                        amounts: None,
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
                        amounts: None,
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
                        amounts: None,
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

            let transfer = entities::near_token_events::ActiveModel {
                id: NotSet,
                contract_id: Set(event.contract_id.clone()),
                token_standard: Set(event.token_standard.clone()),
                event_type: Set(event.event_type.clone()),
                from_account_id: Set(event.from_account_id.clone()),
                to_account_id: Set(event.to_account_id.clone()),
                amount: Set(amount_decimal),
                token_ids: Set(event.token_ids.clone()),
                amounts: Set(event.amounts.clone()),
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
            use entities::near_token_events::Column as TTC;
            use sea_orm::sea_query::OnConflict;
            entities::near_token_events::Entity::insert(transfer)
                .on_conflict(
                    OnConflict::columns([TTC::ReceiptHash, TTC::LogIndex, TTC::EventType])
                        // No-op style update to satisfy MySQL (ON DUPLICATE KEY UPDATE)
                        .update_columns([TTC::LogIndex])
                        .to_owned(),
                )
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
    amounts: Option<String>,   // JSON array for NEP-245 batch amounts
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

        // Unified collection via block-wide event collector
        let all_events = lake_events::collect_block_events(msg);
        for ev in all_events {
            let raw_val = serde_json::to_value(&ev.raw).unwrap_or_default();
            let standard = raw_val
                .get("standard")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let version = raw_val
                .get("version")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let event_type = raw_val.get("event").and_then(|v| v.as_str()).unwrap_or("");
            let data = raw_val
                .get("data")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            if let Some(event) = self.parse_token_event_with_executor(
                &serde_json::json!({
                    "standard": standard,
                    "version": version,
                    "event": event_type,
                    "data": data,
                }),
                &ev.executor_id,
            ) {
                self.process_token_events(vec![event], height, ts, &ev.tx_hash, &ev.receipt_id, db)
                    .await?;
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
