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