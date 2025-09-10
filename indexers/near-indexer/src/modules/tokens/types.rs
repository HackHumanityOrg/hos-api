use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::str::FromStr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventLog {
    pub standard: String,
    pub version: String,
    pub event: String,
    pub data: JsonValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenStandard {
    Nep141, // Fungible Token
    Nep171, // Non-Fungible Token
    Nep245, // Multi-Token
}

impl FromStr for TokenStandard {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "nep141" => Ok(Self::Nep141),
            "nep171" => Ok(Self::Nep171),
            "nep245" => Ok(Self::Nep245),
            _ => Err(format!("Unknown token standard: {}", s)),
        }
    }
}

impl TokenStandard {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Nep141 => "nep141",
            Self::Nep171 => "nep171",
            Self::Nep245 => "nep245",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventType {
    // NEP-141 (Fungible Token) events
    FtMint,
    FtBurn,
    FtTransfer,

    // NEP-171 (Non-Fungible Token) events
    NftMint,
    NftBurn,
    NftTransfer,

    // NEP-245 (Multi-Token) events
    MtMint,
    MtBurn,
    MtTransfer,
}

impl FromStr for EventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "ft_mint" => Ok(Self::FtMint),
            "ft_burn" => Ok(Self::FtBurn),
            "ft_transfer" => Ok(Self::FtTransfer),
            "nft_mint" => Ok(Self::NftMint),
            "nft_burn" => Ok(Self::NftBurn),
            "nft_transfer" => Ok(Self::NftTransfer),
            "mt_mint" => Ok(Self::MtMint),
            "mt_burn" => Ok(Self::MtBurn),
            "mt_transfer" => Ok(Self::MtTransfer),
            _ => Err(format!("Unknown event type: {}", s)),
        }
    }
}

impl EventType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::FtMint => "ft_mint",
            Self::FtBurn => "ft_burn",
            Self::FtTransfer => "ft_transfer",
            Self::NftMint => "nft_mint",
            Self::NftBurn => "nft_burn",
            Self::NftTransfer => "nft_transfer",
            Self::MtMint => "mt_mint",
            Self::MtBurn => "mt_burn",
            Self::MtTransfer => "mt_transfer",
        }
    }

    pub fn is_transfer(&self) -> bool {
        matches!(
            self,
            Self::FtTransfer | Self::NftTransfer | Self::MtTransfer
        )
    }

    pub fn is_mint(&self) -> bool {
        matches!(self, Self::FtMint | Self::NftMint | Self::MtMint)
    }

    pub fn is_burn(&self) -> bool {
        matches!(self, Self::FtBurn | Self::NftBurn | Self::MtBurn)
    }
}

// NEP-141 Fungible Token Events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtTransferData {
    pub old_owner_id: String,
    pub new_owner_id: String,
    pub amount: String,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtMintData {
    pub owner_id: String,
    pub amount: String,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FtBurnData {
    pub owner_id: String,
    pub amount: String,
    pub memo: Option<String>,
}

// NEP-171 Non-Fungible Token Events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftTransferData {
    pub old_owner_id: String,
    pub new_owner_id: String,
    pub token_ids: Vec<String>,
    pub memo: Option<String>,
    pub authorized_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftMintData {
    pub owner_id: String,
    pub token_ids: Vec<String>,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftBurnData {
    pub owner_id: String,
    pub token_ids: Vec<String>,
    pub memo: Option<String>,
    pub authorized_id: Option<String>,
}

// NEP-245 Multi-Token Events
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtTransferData {
    pub old_owner_id: String,
    pub new_owner_id: String,
    pub token_ids: Vec<String>,
    pub amounts: Vec<String>,
    pub memo: Option<String>,
    pub authorized_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtMintData {
    pub owner_id: String,
    pub token_ids: Vec<String>,
    pub amounts: Vec<String>,
    pub memo: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MtBurnData {
    pub owner_id: String,
    pub token_ids: Vec<String>,
    pub amounts: Vec<String>,
    pub memo: Option<String>,
    pub authorized_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedTokenEvent {
    pub standard: TokenStandard,
    pub event_type: EventType,
    pub data: JsonValue,
    pub affected_accounts: Vec<String>,
}

impl ParsedTokenEvent {
    pub fn new(standard: TokenStandard, event_type: EventType, data: JsonValue) -> Self {
        let affected_accounts = Self::extract_affected_accounts(&standard, &event_type, &data);
        Self {
            standard,
            event_type,
            data,
            affected_accounts,
        }
    }

    fn extract_affected_accounts(
        standard: &TokenStandard,
        event_type: &EventType,
        data: &JsonValue,
    ) -> Vec<String> {
        let mut accounts = Vec::new();

        match (standard, event_type) {
            // NEP-141 events
            (TokenStandard::Nep141, EventType::FtTransfer) => {
                if let (Some(old_owner), Some(new_owner)) = (
                    data.get("old_owner_id").and_then(|v| v.as_str()),
                    data.get("new_owner_id").and_then(|v| v.as_str()),
                ) {
                    accounts.push(old_owner.to_string());
                    accounts.push(new_owner.to_string());
                }
            }
            (TokenStandard::Nep141, EventType::FtMint | EventType::FtBurn) => {
                if let Some(owner) = data.get("owner_id").and_then(|v| v.as_str()) {
                    accounts.push(owner.to_string());
                }
            }

            // NEP-171 events
            (TokenStandard::Nep171, EventType::NftTransfer) => {
                if let (Some(old_owner), Some(new_owner)) = (
                    data.get("old_owner_id").and_then(|v| v.as_str()),
                    data.get("new_owner_id").and_then(|v| v.as_str()),
                ) {
                    accounts.push(old_owner.to_string());
                    accounts.push(new_owner.to_string());
                }
            }
            (TokenStandard::Nep171, EventType::NftMint | EventType::NftBurn) => {
                if let Some(owner) = data.get("owner_id").and_then(|v| v.as_str()) {
                    accounts.push(owner.to_string());
                }
            }

            // NEP-245 events
            (TokenStandard::Nep245, EventType::MtTransfer) => {
                if let (Some(old_owner), Some(new_owner)) = (
                    data.get("old_owner_id").and_then(|v| v.as_str()),
                    data.get("new_owner_id").and_then(|v| v.as_str()),
                ) {
                    accounts.push(old_owner.to_string());
                    accounts.push(new_owner.to_string());
                }
            }
            (TokenStandard::Nep245, EventType::MtMint | EventType::MtBurn) => {
                if let Some(owner) = data.get("owner_id").and_then(|v| v.as_str()) {
                    accounts.push(owner.to_string());
                }
            }

            _ => {}
        }

        // Remove duplicates and return
        accounts.sort();
        accounts.dedup();
        accounts
    }
}
