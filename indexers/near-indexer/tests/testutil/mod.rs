use near_crypto::{KeyType, Signature};
use near_lake_framework::near_indexer_primitives as nip;
use nip::views as v;

pub fn make_block(height: u64, ts: u64) -> nip::StreamerMessage {
    let zero = nip::CryptoHash::default();
    let header = v::BlockHeaderView {
        height,
        prev_height: Some(height.saturating_sub(1)),
        epoch_id: zero,
        next_epoch_id: zero,
        hash: zero,
        prev_hash: zero,
        prev_state_root: zero,
        block_body_hash: Some(zero),
        chunk_receipts_root: zero,
        chunk_headers_root: zero,
        chunk_tx_root: zero,
        outcome_root: zero,
        chunks_included: 0,
        challenges_root: zero,
        timestamp: ts,
        timestamp_nanosec: ts,
        random_value: zero,
        validator_proposals: vec![],
        chunk_mask: vec![],
        gas_price: 0,
        block_ordinal: Some(height),
        rent_paid: 0,
        validator_reward: 0,
        total_supply: 0,
        challenges_result: vec![],
        last_final_block: zero,
        last_ds_final_block: zero,
        next_bp_hash: zero,
        block_merkle_root: zero,
        epoch_sync_data_hash: None,
        approvals: vec![],
        signature: Signature::empty(KeyType::ED25519),
        latest_protocol_version: 0,
        chunk_endorsements: None,
    };
    let author: nip::types::AccountId = "test.near".parse().unwrap();
    let block = v::BlockView {
        author,
        header,
        chunks: vec![],
    };
    nip::StreamerMessage {
        block,
        shards: vec![],
    }
}

#[allow(dead_code)]
pub fn outcome_with_logs(executor: &str, logs: Vec<&str>) -> v::ExecutionOutcomeWithIdView {
    v::ExecutionOutcomeWithIdView {
        proof: vec![],
        block_hash: nip::CryptoHash::default(),
        id: nip::CryptoHash::default(),
        outcome: v::ExecutionOutcomeView {
            logs: logs.into_iter().map(|s| s.to_string()).collect(),
            receipt_ids: vec![],
            gas_burnt: 0,
            tokens_burnt: 0,
            executor_id: executor.parse().unwrap(),
            status: v::ExecutionStatusView::Unknown,
            metadata: Default::default(),
        },
    }
}

#[allow(dead_code)]
pub fn make_chunk_with_producer(
    height: u64,
    shard_id: u64,
    producer: &str,
    validator_reward: u128,
) -> nip::IndexerChunkView {
    let header = v::ChunkHeaderView {
        chunk_hash: nip::CryptoHash::default(),
        prev_block_hash: nip::CryptoHash::default(),
        outcome_root: nip::CryptoHash::default(),
        prev_state_root: nip::CryptoHash::default(),
        encoded_merkle_root: nip::CryptoHash::default(),
        encoded_length: 0,
        height_created: height,
        height_included: height,
        shard_id: shard_id.into(),
        gas_used: 0,
        gas_limit: 0,
        rent_paid: 0,
        validator_reward,
        balance_burnt: 0,
        outgoing_receipts_root: nip::CryptoHash::default(),
        tx_root: nip::CryptoHash::default(),
        validator_proposals: vec![],
        congestion_info: None,
        bandwidth_requests: None,
        signature: near_crypto::Signature::empty(near_crypto::KeyType::ED25519),
    };

    nip::IndexerChunkView {
        author: producer.parse().unwrap(),
        header,
        transactions: vec![],
        receipts: vec![],
    }
}

#[allow(dead_code)]
pub fn chunk_header(height: u64, shard_id: u64) -> v::ChunkHeaderView {
    v::ChunkHeaderView {
        chunk_hash: nip::CryptoHash::default(),
        prev_block_hash: nip::CryptoHash::default(),
        outcome_root: nip::CryptoHash::default(),
        prev_state_root: nip::CryptoHash::default(),
        encoded_merkle_root: nip::CryptoHash::default(),
        encoded_length: 0,
        height_created: height,
        height_included: height,
        shard_id: shard_id.into(),
        gas_used: 0,
        gas_limit: 0,
        rent_paid: 0,
        validator_reward: 0,
        balance_burnt: 0,
        outgoing_receipts_root: nip::CryptoHash::default(),
        tx_root: nip::CryptoHash::default(),
        validator_proposals: vec![],
        congestion_info: None,
        bandwidth_requests: None,
        signature: Signature::empty(KeyType::ED25519),
    }
}
