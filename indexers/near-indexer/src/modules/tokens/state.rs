use anyhow::{anyhow, Result};
use entities::*;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};
use serde_json::Value as JsonValue;
use tracing::{debug, warn};

use crate::modules::tokens::types::{EventType, ParsedTokenEvent, TokenStandard};

pub struct TokenStateManager;

impl TokenStateManager {
    /// Update token balances based on a parsed event
    /// Inserts/updates records one by one as requested (no batching)
    pub async fn update_balances_from_event(
        event: &ParsedTokenEvent,
        contract_id: &str,
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        match event.event_type {
            EventType::FtTransfer | EventType::NftTransfer | EventType::MtTransfer => {
                Self::handle_transfer_event(event, contract_id, block_height, db).await
            }
            EventType::FtMint | EventType::NftMint | EventType::MtMint => {
                Self::handle_mint_event(event, contract_id, block_height, db).await
            }
            EventType::FtBurn | EventType::NftBurn | EventType::MtBurn => {
                Self::handle_burn_event(event, contract_id, block_height, db).await
            }
        }
    }

    async fn handle_transfer_event(
        event: &ParsedTokenEvent,
        contract_id: &str,
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let (from_account, to_account, transfers) = Self::extract_transfer_data(event)?;

        // Process each transfer individually (for NEP-245 parallel arrays)
        for transfer in transfers {
            // Update sender balance (decrease)
            if let Some(ref from) = from_account {
                Self::update_balance(
                    contract_id,
                    transfer.token_type_id.as_deref(),
                    transfer.token_id.as_deref(),
                    from,
                    &transfer.amount,
                    false, // decrease
                    &event.standard,
                    block_height,
                    transfer.metadata.as_ref(),
                    db,
                )
                .await?;
            }

            // Update receiver balance (increase)
            if let Some(ref to) = to_account {
                Self::update_balance(
                    contract_id,
                    transfer.token_type_id.as_deref(),
                    transfer.token_id.as_deref(),
                    to,
                    &transfer.amount,
                    true, // increase
                    &event.standard,
                    block_height,
                    transfer.metadata.as_ref(),
                    db,
                )
                .await?;
            }
        }

        Ok(())
    }

    async fn handle_mint_event(
        event: &ParsedTokenEvent,
        contract_id: &str,
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let (owner_id, mints) = Self::extract_mint_data(event)?;

        // Process each mint individually
        for mint in mints {
            Self::update_balance(
                contract_id,
                mint.token_type_id.as_deref(),
                mint.token_id.as_deref(),
                &owner_id,
                &mint.amount,
                true, // increase
                &event.standard,
                block_height,
                mint.metadata.as_ref(),
                db,
            )
            .await?;
        }

        Ok(())
    }

    async fn handle_burn_event(
        event: &ParsedTokenEvent,
        contract_id: &str,
        block_height: i64,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        let (owner_id, burns) = Self::extract_burn_data(event)?;

        // Process each burn individually
        for burn in burns {
            Self::update_balance(
                contract_id,
                burn.token_type_id.as_deref(),
                burn.token_id.as_deref(),
                &owner_id,
                &burn.amount,
                false, // decrease
                &event.standard,
                block_height,
                burn.metadata.as_ref(),
                db,
            )
            .await?;
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn update_balance(
        contract_id: &str,
        token_type_id: Option<&str>,
        token_id: Option<&str>,
        account_id: &str,
        amount: &str,
        is_increase: bool,
        standard: &TokenStandard,
        block_height: i64,
        metadata: Option<&JsonValue>,
        db: &impl ConnectionTrait,
    ) -> Result<()> {
        // For NFTs and SBTs, amount is always "1" or "0"
        let amount_value = match standard {
            TokenStandard::Nep171 => {
                if is_increase {
                    "1"
                } else {
                    "0"
                }
            }
            _ => amount,
        };

        // Check if balance exists
        let existing_balance = near_token_balances::Entity::find()
            .filter(near_token_balances::Column::ContractId.eq(contract_id))
            .filter(near_token_balances::Column::TokenTypeId.eq(token_type_id.unwrap_or("")))
            .filter(near_token_balances::Column::TokenId.eq(token_id.unwrap_or("")))
            .filter(near_token_balances::Column::AccountId.eq(account_id))
            .one(db)
            .await?;

        let balance_record = if let Some(existing) = existing_balance {
            // Update existing balance
            let current_amount = existing.amount;
            let new_amount = if is_increase {
                Self::add_amounts(&current_amount, amount_value)?
            } else {
                Self::subtract_amounts(&current_amount, amount_value)?
            };

            near_token_balances::ActiveModel {
                contract_id: Set(contract_id.to_string()),
                token_type_id: Set(token_type_id.unwrap_or("").to_string()),
                token_id: Set(token_id.unwrap_or("").to_string()),
                account_id: Set(account_id.to_string()),
                amount: Set(new_amount),
                standard: Set(standard.as_str().to_string()),
                last_updated_block: Set(block_height),
                metadata: Set(metadata.cloned()),
            }
        } else {
            // Create new balance record
            let initial_amount = if is_increase {
                amount_value.to_string()
            } else {
                // This should not happen for new records, but handle gracefully
                warn!(
                    "Attempting to decrease non-existent balance for {}/{}",
                    contract_id, account_id
                );
                "0".to_string()
            };

            near_token_balances::ActiveModel {
                contract_id: Set(contract_id.to_string()),
                token_type_id: Set(token_type_id.unwrap_or("").to_string()),
                token_id: Set(token_id.unwrap_or("").to_string()),
                account_id: Set(account_id.to_string()),
                amount: Set(initial_amount),
                standard: Set(standard.as_str().to_string()),
                last_updated_block: Set(block_height),
                metadata: Set(metadata.cloned()),
            }
        };

        // Insert or update (upsert)
        balance_record.save(db).await?;

        debug!(
            "Updated balance for {}/{}: {} {} {}",
            contract_id,
            account_id,
            if is_increase { "+" } else { "-" },
            amount_value,
            token_id.unwrap_or("FT")
        );

        Ok(())
    }

    // String arithmetic for handling U128 amounts
    fn add_amounts(a: &str, b: &str) -> Result<String> {
        let a_num: u128 = a.parse().map_err(|_| anyhow!("Invalid amount: {}", a))?;
        let b_num: u128 = b.parse().map_err(|_| anyhow!("Invalid amount: {}", b))?;
        let result = a_num
            .checked_add(b_num)
            .ok_or_else(|| anyhow!("Amount overflow when adding {} + {}", a, b))?;
        Ok(result.to_string())
    }

    fn subtract_amounts(a: &str, b: &str) -> Result<String> {
        let a_num: u128 = a.parse().map_err(|_| anyhow!("Invalid amount: {}", a))?;
        let b_num: u128 = b.parse().map_err(|_| anyhow!("Invalid amount: {}", b))?;
        let result = a_num
            .checked_sub(b_num)
            .ok_or_else(|| anyhow!("Amount underflow when subtracting {} - {}", a, b))?;
        Ok(result.to_string())
    }

    // Extract transfer data from event
    fn extract_transfer_data(
        event: &ParsedTokenEvent,
    ) -> Result<(Option<String>, Option<String>, Vec<TransferData>)> {
        let data = &event.data;

        let from_account = data
            .get("old_owner_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let to_account = data
            .get("new_owner_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let transfers = match event.standard {
            TokenStandard::Nep141 => {
                let amount = data
                    .get("amount")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0")
                    .to_string();

                vec![TransferData {
                    token_type_id: None,
                    token_id: None,
                    amount,
                    metadata: None,
                }]
            }
            TokenStandard::Nep171 => {
                let empty_vec = vec![];
                let token_ids = data
                    .get("token_ids")
                    .and_then(|v| v.as_array())
                    .unwrap_or(&empty_vec);

                token_ids
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|token_id| TransferData {
                        token_type_id: None,
                        token_id: Some(token_id.to_string()),
                        amount: "1".to_string(), // NFTs always have amount 1
                        metadata: None,
                    })
                    .collect()
            }
            TokenStandard::Nep245 => {
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

                token_ids
                    .iter()
                    .zip(amounts.iter())
                    .filter_map(|(tid, amt)| {
                        let token_id = tid.as_str()?;
                        let amount = amt.as_str()?;

                        // Parse token_type_id from token_id if it contains ":"
                        let (token_type_id, final_token_id) = if token_id.contains(':') {
                            let parts: Vec<&str> = token_id.splitn(2, ':').collect();
                            (Some(parts[0].to_string()), Some(parts[1].to_string()))
                        } else {
                            (None, Some(token_id.to_string()))
                        };

                        Some(TransferData {
                            token_type_id,
                            token_id: final_token_id,
                            amount: amount.to_string(),
                            metadata: None,
                        })
                    })
                    .collect()
            }
        };

        Ok((from_account, to_account, transfers))
    }

    fn extract_mint_data(event: &ParsedTokenEvent) -> Result<(String, Vec<TransferData>)> {
        let data = &event.data;

        let owner_id = data
            .get("owner_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing owner_id in mint event"))?
            .to_string();

        let mints = match event.standard {
            TokenStandard::Nep141 => {
                let amount = data
                    .get("amount")
                    .and_then(|v| v.as_str())
                    .unwrap_or("0")
                    .to_string();

                vec![TransferData {
                    token_type_id: None,
                    token_id: None,
                    amount,
                    metadata: None,
                }]
            }
            TokenStandard::Nep171 => {
                let empty_vec = vec![];
                let token_ids = data
                    .get("token_ids")
                    .and_then(|v| v.as_array())
                    .unwrap_or(&empty_vec);

                token_ids
                    .iter()
                    .filter_map(|v| v.as_str())
                    .map(|token_id| TransferData {
                        token_type_id: None,
                        token_id: Some(token_id.to_string()),
                        amount: "1".to_string(),
                        metadata: None,
                    })
                    .collect()
            }
            TokenStandard::Nep245 => {
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

                token_ids
                    .iter()
                    .zip(amounts.iter())
                    .filter_map(|(tid, amt)| {
                        let token_id = tid.as_str()?;
                        let amount = amt.as_str()?;

                        let (token_type_id, final_token_id) = if token_id.contains(':') {
                            let parts: Vec<&str> = token_id.splitn(2, ':').collect();
                            (Some(parts[0].to_string()), Some(parts[1].to_string()))
                        } else {
                            (None, Some(token_id.to_string()))
                        };

                        Some(TransferData {
                            token_type_id,
                            token_id: final_token_id,
                            amount: amount.to_string(),
                            metadata: None,
                        })
                    })
                    .collect()
            }
        };

        Ok((owner_id, mints))
    }

    fn extract_burn_data(event: &ParsedTokenEvent) -> Result<(String, Vec<TransferData>)> {
        // Burn data has the same structure as mint data
        Self::extract_mint_data(event)
    }
}

#[derive(Debug, Clone)]
struct TransferData {
    token_type_id: Option<String>,
    token_id: Option<String>,
    amount: String,
    metadata: Option<JsonValue>,
}
