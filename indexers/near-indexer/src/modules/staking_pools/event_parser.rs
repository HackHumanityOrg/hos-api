use anyhow::{anyhow, Result};
use serde_json;

use crate::modules::staking_pools::types::StakingEvent;

const EVENT_JSON_PREFIX: &str = "EVENT_JSON:";

/// Parse events from staking pool contract logs
pub struct EventParser;

impl EventParser {
    /// Parse events from execution outcome logs
    pub fn parse_events(
        logs: &[String],
        pool_id: &str,
        execution_outcome_id: &str,
        block_height: u64,
        timestamp: i64,
    ) -> Vec<StakingEvent> {
        let mut events = Vec::new();

        for (index, log) in logs.iter().enumerate() {
            // Try to parse as structured event
            if let Ok(event) = Self::parse_structured_event(
                log,
                pool_id,
                execution_outcome_id,
                index as i32,
                block_height.try_into().unwrap(),
                timestamp,
            ) {
                events.push(event);
            }
            // Try to parse as custom staking event
            else if let Ok(event) = Self::parse_custom_event(
                log,
                pool_id,
                execution_outcome_id,
                index as i32,
                block_height.try_into().unwrap(),
                timestamp,
            ) {
                events.push(event);
            }
            // Store as raw event if we can't parse it but it looks relevant
            else if Self::looks_like_staking_log(log) {
                if let Ok(pool_account_id) = pool_id.parse() {
                    events.push(StakingEvent {
                        pool_id: pool_account_id,
                        event_standard: None,
                        event_type: "raw_log".to_string(),
                        event_data: serde_json::json!({
                            "message": log,
                            "parsed": false
                        }),
                        raw_log: log.clone(),
                        log_index: index as i32,
                        execution_outcome_id: execution_outcome_id.to_string(),
                        block_height,
                        timestamp,
                    });
                }
            }
        }

        events
    }

    /// Parse NEP-141 style structured events (EVENT_JSON:...)
    fn parse_structured_event(
        log: &str,
        pool_id: &str,
        execution_outcome_id: &str,
        log_index: i32,
        block_height: u64,
        timestamp: i64,
    ) -> Result<StakingEvent> {
        // Check if log starts with EVENT_JSON prefix
        let json_str = log
            .strip_prefix(EVENT_JSON_PREFIX)
            .ok_or_else(|| anyhow!("Log does not start with EVENT_JSON prefix"))?;

        // Parse the JSON
        let event_log: serde_json::Value = serde_json::from_str(json_str)
            .map_err(|e| anyhow!("Failed to parse EVENT_JSON: {}", e))?;

        let event_standard = event_log
            .get("standard")
            .and_then(|s| s.as_str())
            .map(|s| s.to_string());

        let event_type = event_log
            .get("event")
            .and_then(|e| e.as_str())
            .ok_or_else(|| anyhow!("Missing event type"))?
            .to_string();

        let event_data = event_log
            .get("data")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        Ok(StakingEvent {
            pool_id: pool_id.parse().map_err(|e| anyhow!("Invalid AccountId: {}", e))?,
            event_standard,
            event_type,
            event_data,
            raw_log: log.to_string(),
            log_index,
            execution_outcome_id: execution_outcome_id.to_string(),
            block_height,
            timestamp,
        })
    }

    /// Parse custom staking pool events (non-standard format)
    fn parse_custom_event(
        log: &str,
        pool_id: &str,
        execution_outcome_id: &str,
        log_index: i32,
        block_height: u64,
        timestamp: i64,
    ) -> Result<StakingEvent> {
        // Try to parse common staking pool log patterns
        if log.contains("deposit") || log.contains("stake") || log.contains("unstake") || log.contains("withdraw") {
            // Try to parse as JSON first
            if let Ok(json_data) = serde_json::from_str::<serde_json::Value>(log) {
                return Ok(StakingEvent {
                    pool_id: pool_id.parse().map_err(|e| anyhow!("Invalid AccountId: {}", e))?,
                    event_standard: Some("custom".to_string()),
                    event_type: Self::extract_event_type_from_log(log),
                    event_data: json_data,
                    raw_log: log.to_string(),
                    log_index,
                    execution_outcome_id: execution_outcome_id.to_string(),
                    block_height,
                    timestamp,
                });
            }
        }

        Err(anyhow!("Not a custom staking event"))
    }

    /// Extract event type from log content
    fn extract_event_type_from_log(log: &str) -> String {
        let log_lower = log.to_lowercase();
        if log_lower.contains("deposit_and_stake") {
            "deposit_and_stake".to_string()
        } else if log_lower.contains("deposit") {
            "deposit".to_string()
        } else if log_lower.contains("unstake") {
            "unstake".to_string()
        } else if log_lower.contains("stake") || log_lower.contains("staking") {
            "stake".to_string()
        } else if log_lower.contains("withdraw") {
            "withdraw".to_string()
        } else if log_lower.contains("ping") {
            "ping".to_string()
        } else {
            "unknown".to_string()
        }
    }

    /// Check if a log looks like it might be staking-related
    fn looks_like_staking_log(log: &str) -> bool {
        let staking_keywords = [
            "stake", "unstake", "deposit", "withdraw", "ping", 
            "delegation", "reward", "validator", "pool"
        ];

        staking_keywords.iter().any(|&keyword| log.to_lowercase().contains(keyword))
    }

    /// Parse reward distribution events from logs
    pub fn parse_reward_events(
        logs: &[String],
        _pool_id: &str,
        _execution_outcome_id: &str,
        _block_height: u64,
        _timestamp: i64,
    ) -> Vec<(usize, String, Option<String>)> {
        let mut reward_events = Vec::new();

        for (index, log) in logs.iter().enumerate() {
            let log_lower = log.to_lowercase();
            
            // Look for reward distribution patterns
            if log_lower.contains("reward") && (log_lower.contains("distributed") || log_lower.contains("distribution")) {
                // Try to extract amount from patterns like "Rewards distributed: 100.5 NEAR"
                let amount = Self::extract_amount_from_log(log);
                reward_events.push((index, log.clone(), amount));
            }
            // Look for ping-related reward updates
            else if log_lower.contains("ping") && log_lower.contains("reward") {
                let amount = Self::extract_amount_from_log(log);
                reward_events.push((index, log.clone(), amount));
            }
            // Look for JSON reward events
            else if log.starts_with("EVENT_JSON:") {
                if let Ok(event_log) = serde_json::from_str::<serde_json::Value>(&log[11..]) {
                    if let Some(event_type) = event_log.get("event").and_then(|e| e.as_str()) {
                        if event_type == "reward" || event_type == "distribute_rewards" {
                            let amount = event_log.get("data")
                                .and_then(|data| data.get("amount"))
                                .and_then(|a| a.as_str())
                                .map(|s| s.to_string());
                            reward_events.push((index, log.clone(), amount));
                        }
                    }
                }
            }
        }

        reward_events
    }

    /// Parse balance change events from logs
    pub fn parse_balance_events(
        logs: &[String],
        _pool_id: &str,
        _execution_outcome_id: &str,
        _block_height: u64,
        _timestamp: i64,
    ) -> Vec<(usize, String, String, String, String)> { // (index, log, account_id, balance_type, balance)
        let mut balance_events = Vec::new();

        for (index, log) in logs.iter().enumerate() {
            let log_lower = log.to_lowercase();

            // Look for balance update patterns
            if log_lower.contains("balance") && (log_lower.contains("update") || log_lower.contains("changed")) {
                // Try to parse patterns like "Balance updated: alice.near staked: 100, unstaked: 50"
                if let Some((account_id, staked, unstaked)) = Self::parse_balance_update_log(log) {
                    balance_events.push((index, log.clone(), account_id.clone(), "staked".to_string(), staked));
                    if !unstaked.is_empty() && unstaked != "0" {
                        balance_events.push((index, log.clone(), account_id, "unstaked".to_string(), unstaked));
                    }
                }
            }
            // Look for specific staked/unstaked amount mentions
            else if (log_lower.contains("staked:") || log_lower.contains("unstaked:")) && log_lower.contains("near") {
                if let Some((account_id, balance_type, amount)) = Self::parse_stake_amount_log(log) {
                    balance_events.push((index, log.clone(), account_id, balance_type, amount));
                }
            }
            // Look for JSON balance events
            else if log.starts_with("EVENT_JSON:") {
                if let Ok(event_log) = serde_json::from_str::<serde_json::Value>(&log[11..]) {
                    if let Some(event_type) = event_log.get("event").and_then(|e| e.as_str()) {
                        if event_type == "balance_update" || event_type == "account_update" {
                            if let Some(data) = event_log.get("data") {
                                let account_id = data.get("account_id").and_then(|a| a.as_str()).unwrap_or("unknown").to_string();
                                
                                if let Some(staked) = data.get("staked").and_then(|s| s.as_str()) {
                                    balance_events.push((index, log.clone(), account_id.clone(), "staked".to_string(), staked.to_string()));
                                }
                                if let Some(unstaked) = data.get("unstaked").and_then(|s| s.as_str()) {
                                    if unstaked != "0" {
                                        balance_events.push((index, log.clone(), account_id, "unstaked".to_string(), unstaked.to_string()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        balance_events
    }

    /// Extract amount from log text using regex patterns
    fn extract_amount_from_log(log: &str) -> Option<String> {
        // Look for patterns like "100.5 NEAR", "1000000000000000000000000" (yoctoNEAR)
        
        // Pattern 1: Amount followed by NEAR
        if let Some(start) = log.find(char::is_numeric) {
            let amount_part = &log[start..];
            if let Some(end) = amount_part.find("near") {
                let amount_str = amount_part[..end].trim();
                return Some(amount_str.to_string());
            }
        }

        // Pattern 2: Look for large numbers (yoctoNEAR amounts)
        for word in log.split_whitespace() {
            if word.len() > 15 && word.chars().all(|c| c.is_ascii_digit()) {
                return Some(word.to_string());
            }
        }

        None
    }

    /// Parse balance update log like "Balance updated: alice.near staked: 100, unstaked: 50"
    fn parse_balance_update_log(log: &str) -> Option<(String, String, String)> {
        // Simple regex-like parsing for balance updates
        if let Some(account_start) = log.find("alice.near").or_else(|| log.find(".near")) {
            // This is a simplified parser - in practice you'd want more robust parsing
            let account_part = &log[..account_start + ".near".len()];
            if let Some(acc_start) = account_part.rfind(' ') {
                let account_id = account_part[acc_start + 1..].to_string();
                
                let staked = Self::extract_amount_after_keyword(log, "staked:").unwrap_or("0".to_string());
                let unstaked = Self::extract_amount_after_keyword(log, "unstaked:").unwrap_or("0".to_string());
                
                return Some((account_id, staked, unstaked));
            }
        }
        None
    }

    /// Parse stake amount log like "alice.near staked: 100 NEAR"
    fn parse_stake_amount_log(log: &str) -> Option<(String, String, String)> {
        // Look for account ID
        if let Some(near_pos) = log.find(".near") {
            let account_end = near_pos + ".near".len();
            let account_start = log[..near_pos].rfind(' ').map(|i| i + 1).unwrap_or(0);
            let account_id = log[account_start..account_end].to_string();

            // Determine if it's staked or unstaked
            let balance_type = if log.to_lowercase().contains("staked:") {
                "staked"
            } else if log.to_lowercase().contains("unstaked:") {
                "unstaked"
            } else {
                return None;
            };

            // Extract amount
            if let Some(amount) = Self::extract_amount_from_log(log) {
                return Some((account_id, balance_type.to_string(), amount));
            }
        }
        None
    }

    /// Extract amount after a specific keyword
    fn extract_amount_after_keyword(log: &str, keyword: &str) -> Option<String> {
        if let Some(keyword_pos) = log.to_lowercase().find(keyword) {
            let after_keyword = &log[keyword_pos + keyword.len()..];
            if let Some(amount) = Self::extract_amount_from_log(after_keyword) {
                return Some(amount);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_structured_event() {
        let log = r#"EVENT_JSON:{"standard":"staking","version":"1.0.0","event":"deposit","data":{"account_id":"alice.near","amount":"1000000000000000000000000"}}"#;
        
        let events = EventParser::parse_events(
            &[log.to_string()], 
            "pool.near", 
            "outcome123", 
            100, 
            1234567890
        );
        
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "deposit");
        assert_eq!(events[0].event_standard, Some("staking".to_string()));
    }

    #[test]
    fn test_extract_event_type() {
        assert_eq!(EventParser::extract_event_type_from_log("user deposited 100 NEAR"), "deposit");
        assert_eq!(EventParser::extract_event_type_from_log("staking 50 NEAR"), "stake");
        assert_eq!(EventParser::extract_event_type_from_log("unstake request"), "unstake");
        assert_eq!(EventParser::extract_event_type_from_log("withdraw completed"), "withdraw");
    }

    #[test]
    fn test_looks_like_staking_log() {
        assert!(EventParser::looks_like_staking_log("User deposited 100 NEAR"));
        assert!(EventParser::looks_like_staking_log("Staking rewards distributed"));
        assert!(EventParser::looks_like_staking_log("Validator updated"));
        assert!(!EventParser::looks_like_staking_log("Random log message"));
    }
}