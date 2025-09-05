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
    pub timestamp_ns: i64,
    pub hash: String,
    pub epoch_id: String,
    pub gas_price: u128,
    pub total_supply: u128,
    pub protocol_version: u32,
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
            timestamp_ns: h.timestamp_nanosec as i64,
            hash: h.hash.to_string(),
            epoch_id: h.epoch_id.to_string(),
            gas_price: h.gas_price,
            total_supply: h.total_supply,
            protocol_version: h.latest_protocol_version,
        }
    }
}
