use near_primitives::types::{AccountId, Balance, BlockHeight, EpochHeight, EpochId};
use near_primitives::views::validator_stake_view::ValidatorStakeView;
use serde_json;

/// Pool types based on detection method
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PoolType {
    PoolV1, // *.poolv1.near
    Pool,   // *.pool.near
    Custom, // Custom pool specified in config
}

impl PoolType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PoolType::PoolV1 => "poolv1",
            PoolType::Pool => "pool",
            PoolType::Custom => "custom",
        }
    }
}

/// Detection method for pools
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DetectionMethod {
    Pattern,    // Detected by account ID pattern
    MethodCall, // Detected by receiving staking method calls
    Config,     // Specified in custom pools config
}

impl DetectionMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            DetectionMethod::Pattern => "pattern",
            DetectionMethod::MethodCall => "method_call",
            DetectionMethod::Config => "config",
        }
    }
}

/// Staking-related actions from blockchain data
/// Note: StakeAction is now handled via querying near_transactions table directly
/// KeyChange is handled via near_access_keys table in accounts module
#[derive(Debug, Clone)]
pub enum StakingAction {
    /// Function call to staking pool (the only action we store directly)
    PoolCall {
        pool_id: AccountId,
        method_name: String,
        caller_account_id: AccountId,
        args_base64: String,
        args_json: Option<serde_json::Value>,
        attached_deposit: Balance,
        transaction_hash: String,
        receipt_id: String,
        block_height: BlockHeight,
        timestamp: i64,
        success: Option<bool>,
    },
}

/// Parsed events from staking pools
#[derive(Debug, Clone)]
pub struct StakingEvent {
    pub pool_id: AccountId,
    pub event_standard: Option<String>,
    pub event_type: String,
    pub event_data: serde_json::Value,
    pub raw_log: String,
    pub log_index: i32,
    pub execution_outcome_id: String,
    pub block_height: BlockHeight,
    pub timestamp: i64,
}

/// Context for processing actions
#[derive(Debug, Clone)]
pub struct ActionContext {
    pub predecessor_id: AccountId,
    pub receiver_id: AccountId,
    pub transaction_hash: String,
    pub receipt_id: String,
    pub block_height: BlockHeight,
    pub timestamp: i64,
}

/// Epoch information aligned with near_primitives::epoch_info::EpochInfo
#[derive(Debug, Clone)]
pub struct StakingEpoch {
    pub epoch_id: EpochId,
    pub epoch_height: EpochHeight,
    pub start_block_height: BlockHeight,
    pub end_block_height: Option<BlockHeight>,
    pub protocol_version: Option<u32>,
    pub total_stake: Balance,
    pub total_validator_reward: Balance,
    pub validators_info: Option<Vec<ValidatorStakeView>>,
    pub chunk_producers_settlement: Option<serde_json::Value>,
    pub epoch_duration_blocks: Option<i64>,
}

/// Standard staking pool method names
pub const STAKING_METHODS: &[&str] = &[
    "deposit",
    "deposit_and_stake",
    "stake",
    "stake_all",
    "unstake",
    "unstake_all",
    "withdraw",
    "withdraw_all",
    "ping",
    "get_account",
    "get_accounts",
    "pause_staking",
    "resume_staking",
    "update_reward_fee_fraction",
];

/// Check if a method name is staking-related
pub fn is_staking_method(method_name: &str) -> bool {
    STAKING_METHODS.contains(&method_name)
}
