use anyhow::{anyhow, Result};
use base64::Engine;
use near_primitives::views::ActionView;
use serde_json;

use crate::modules::staking_pools::types::{is_staking_method, ActionContext, StakingAction};

/// Parse actions from transactions and receipts to extract staking-related data
pub struct ActionParser;

impl ActionParser {
    /// Parse actions from a transaction or receipt to extract staking pool calls
    /// Note: Stake actions and key changes are handled by other modules now
    pub fn parse_actions(
        actions: &[ActionView],
        context: &ActionContext,
        is_known_pool: bool,
    ) -> Vec<StakingAction> {
        let mut staking_actions = Vec::new();

        for action in actions {
            match action {
                ActionView::FunctionCall {
                    method_name,
                    args,
                    deposit,
                    gas: _,
                } => {
                    // Only process staking pool function calls
                    if is_known_pool || is_staking_method(method_name) {
                        let args_bytes: &[u8] = args.as_ref();
                        let args_base64 = base64::prelude::BASE64_STANDARD.encode(args_bytes);
                        let args_json = Self::try_parse_args(args_bytes).ok();

                        staking_actions.push(StakingAction::PoolCall {
                            pool_id: context.receiver_id.clone(),
                            method_name: method_name.clone(),
                            caller_account_id: context.predecessor_id.clone(),
                            args_base64,
                            args_json,
                            attached_deposit: *deposit,
                            transaction_hash: context.transaction_hash.clone(),
                            receipt_id: context.receipt_id.clone(),
                            block_height: context.block_height,
                            timestamp: context.timestamp,
                            success: None, // Will be filled from execution outcome
                        });
                    }
                }
                _ => {
                    // All other action types are handled by other modules:
                    // - Stake actions: Query from near_transactions table
                    // - AddKey/DeleteKey: Handled by accounts module in near_access_keys
                }
            }
        }

        staking_actions
    }

    /// Try to parse base64-encoded arguments as JSON
    fn try_parse_args(args: &[u8]) -> Result<serde_json::Value> {
        if args.is_empty() {
            return Ok(serde_json::Value::Object(serde_json::Map::new()));
        }

        // Try to parse as JSON directly
        match serde_json::from_slice(args) {
            Ok(value) => Ok(value),
            Err(_) => {
                // Try to decode as string first
                match std::str::from_utf8(args) {
                    Ok(s) => serde_json::from_str(s)
                        .map_err(|e| anyhow!("Failed to parse args as JSON: {}", e)),
                    Err(e) => Err(anyhow!("Args are not valid UTF-8: {}", e)),
                }
            }
        }
    }

    // Removed: is_potential_validator_key - key tracking moved to accounts module
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_args() {
        // Test empty args
        let args = b"";
        let result = ActionParser::try_parse_args(args);
        assert!(result.is_ok());

        // Test JSON args
        let args = b"{\"amount\": \"1000000000000000000000000\"}";
        let result = ActionParser::try_parse_args(args);
        assert!(result.is_ok());

        if let Ok(serde_json::Value::Object(map)) = result {
            assert!(map.contains_key("amount"));
        }
    }

    // Removed: test_is_potential_validator_key - functionality moved to accounts module
}
