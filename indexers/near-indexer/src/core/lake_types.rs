use rust_decimal::Decimal;

/// Execution status simplified.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecStatus {
    Success,
    Failure,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct BlockHeaderView {
    pub height: i64,
    pub epoch_id: String,
    pub next_epoch_id: String,
    pub hash: String,
    pub prev_hash: String,
    pub chunks_included: u64,
    pub rent_paid: u128,
    pub validator_reward: u128,
    pub last_final_block: String,
    pub last_ds_final_block: String,
    pub next_bp_hash: String,
    pub block_merkle_root: String,
    pub random_value: String,
    pub timestamp: u64,
    pub timestamp_nanosec: u64,
    pub block_ordinal: Option<u64>,
    pub prev_height: Option<u64>,
    pub gas_price: u128,
    pub protocol_version: u32,
    pub total_supply: u128,
}

#[derive(Debug, Clone)]
pub struct TxView {
    pub hash: String,
    pub signer_id: String,
    pub receiver_id: String,
    pub status: ExecStatus,
    pub gas_burnt: i64,
    pub tokens_burnt: Decimal,
    pub actions_json: String,
    pub receipt_ids: Vec<String>,
    pub executor_id: String,
    pub public_key: Option<String>,
    pub nonce: Option<i64>,
    pub priority_fee: Decimal,
    pub signature: String,
    pub execution_outcome_id: String,
    pub logs: Vec<String>,
    pub proof: Vec<String>,
    pub metadata: String,
}

#[derive(Debug, Clone)]
pub struct ReceiptOutcomeView {
    pub receipt_id: String,
    pub executor_id: String,
    pub logs: Vec<String>,
}

/// Flattened state change with normalized cause and optional linkage.
#[derive(Debug, Clone)]
pub struct StateChangeRecord {
    pub cause: String,
    pub cause_tx: Option<String>,
    pub cause_receipt: Option<String>,
    pub value: near_lake_framework::near_indexer_primitives::views::StateChangeValueView,
}

/// Chunk producer per shard for convenience.
#[derive(Debug, Clone)]
pub struct ChunkProducer {
    pub shard_id: u64,
    pub chunk_hash: Option<String>,
    pub producer: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AdaptedBlock {
    pub header: BlockHeaderView,
    pub txs: Vec<TxView>,
    pub receipt_outcomes: Vec<ReceiptOutcomeView>,
    pub state_changes: Vec<StateChangeRecord>,
    pub chunk_producers: Vec<ChunkProducer>,
}

impl BlockHeaderView {
    pub fn new_from(msg: &near_lake_framework::near_indexer_primitives::StreamerMessage) -> Self {
        let h = &msg.block.header;
        Self {
            height: h.height as i64,
            epoch_id: h.epoch_id.to_string(),
            next_epoch_id: h.next_epoch_id.to_string(),
            hash: h.hash.to_string(),
            prev_hash: h.prev_hash.to_string(),
            chunks_included: h.chunks_included,
            rent_paid: h.rent_paid,
            validator_reward: h.validator_reward,
            last_final_block: h.last_final_block.to_string(),
            last_ds_final_block: h.last_ds_final_block.to_string(),
            next_bp_hash: h.next_bp_hash.to_string(),
            block_merkle_root: h.block_merkle_root.to_string(),
            random_value: h.random_value.to_string(),
            timestamp: h.timestamp,
            timestamp_nanosec: h.timestamp_nanosec,
            block_ordinal: h.block_ordinal,
            prev_height: h.prev_height,
            gas_price: h.gas_price,
            protocol_version: h.latest_protocol_version,
            total_supply: h.total_supply,
        }
    }
}
