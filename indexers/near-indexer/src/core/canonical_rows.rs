use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use near_lake_framework::near_indexer_primitives::views::{
    ActionView, BlockView, ExecutionOutcomeWithIdView, ExecutionStatusView, ReceiptView,
    StateChangesView,
};
use near_lake_framework::near_indexer_primitives::{types::ShardId, IndexerChunkView};
use serde::Serialize;
use serde_json::{json, Value as JsonValue};

#[derive(Clone)]
pub struct ReceiptTxHashMapRow {
    pub receipt_id: String,
    pub transaction_hash: String,
    pub block_height: i64,
}

#[derive(Clone)]
pub struct NearBlockRow {
    pub height: i64,
    pub hash: String,
    pub prev_hash: String,
    pub epoch_id: String,
    pub next_epoch_id: String,
    pub author: String,
    pub timestamp: i64,
    pub timestamp_nanosec: i64,
    pub gas_price: String,
    pub total_supply: String,
    pub validator_reward: String,
    pub protocol_version: i32,
    pub chunks_included: i64,
    pub block_ordinal: Option<i64>,
    pub prev_height: Option<i64>,
    pub raw_data: JsonValue,
}

#[derive(Clone)]
pub struct NearChunkRow {
    pub chunk_hash: String,
    pub block_height: i64,
    pub shard_id: i64,
    pub author: String,
    pub height_created: i64,
    pub height_included: i64,
    pub gas_used: i64,
    pub gas_limit: i64,
    pub validator_reward: String,
    pub raw_data: JsonValue,
}

#[derive(Clone)]
pub struct NearStateChangesRow {
    pub block_height: i64,
    pub shard_id: i64,
    pub state_changes: JsonValue,
}

#[derive(Clone)]
pub struct NearTransactionRow {
    pub hash: String,
    pub block_height: i64,
    pub shard_id: i64,
    pub signer_id: String,
    pub receiver_id: String,
    pub nonce: i64,
    pub priority_fee: i64,
    pub public_key: String,
    pub signature: String,
    pub actions: JsonValue,
    pub raw_data: JsonValue,
}

#[derive(Clone)]
pub struct NearReceiptRow {
    pub receipt_id: String,
    pub block_height: i64,
    pub shard_id: i64,
    pub predecessor_id: String,
    pub receiver_id: String,
    pub receipt: JsonValue,
    pub raw_data: JsonValue,
}

#[derive(Clone)]
pub struct NearExecutionOutcomeRow {
    pub id: String,
    pub transaction_hash: Option<String>,
    pub block_height: i64,
    pub shard_id: i64,
    pub receipt_id: Option<String>,
    pub outcome: JsonValue,
    pub raw_data: JsonValue,
    pub block_hash: String,
    pub chunk_hash: String,
    pub gas_burnt: i64,
    pub gas_used: f64,
    pub tokens_burnt: f64,
    pub executor_account_id: String,
    pub status: String,
    pub outcome_receipt_ids: JsonValue,
    pub executed_in_block_hash: String,
    pub logs: JsonValue,
    pub results_json: Option<JsonValue>,
    pub block_timestamp: i64,
}

#[derive(Clone)]
pub struct NearReceiptActionRow {
    pub id: String,
    pub transaction_hash: Option<String>,
    pub block_height: i64,
    pub receipt_id: String,
    pub execution_outcome_id: String,
    pub shard_id: i64,
    pub signer_account_id: Option<String>,
    pub signer_public_key: Option<String>,
    pub gas_price: Option<String>,
    pub action_kind: String,
    pub action_json: JsonValue,
    pub predecessor_id: String,
    pub receiver_id: String,
    pub block_hash: String,
    pub chunk_hash: String,
    pub author: String,
    pub method_name: Option<String>,
    pub gas: Option<i64>,
    pub deposit: Option<String>,
    pub args_base64: Option<String>,
    pub args_json: Option<JsonValue>,
    pub action_index: i32,
    pub block_timestamp: i64,
}

#[derive(Clone)]
pub struct NearReceiptEdgeRow {
    pub parent_receipt_id: String,
    pub child_receipt_id: String,
    pub parent_execution_outcome_id: String,
    pub block_height: i64,
    pub block_hash: String,
    pub block_timestamp: i64,
    pub transaction_hash: Option<String>,
}

pub struct ExecutionOutcomeRowMeta<'a> {
    pub transaction_hash: Option<&'a str>,
    pub block_height: i64,
    pub shard_id: i64,
    pub receipt_id: Option<&'a str>,
    pub block_hash: &'a str,
    pub chunk_hash: &'a str,
    pub block_timestamp: i64,
}

pub struct ReceiptActionRowMeta<'a> {
    pub transaction_hash: Option<&'a str>,
    pub block_height: i64,
    pub execution_outcome_id: &'a str,
    pub shard_id: i64,
    pub block_hash: &'a str,
    pub chunk_hash: &'a str,
    pub author: &'a str,
    pub block_timestamp: i64,
}

pub fn shard_to_i64(shard_id: ShardId) -> i64 {
    let value: u64 = shard_id.into();
    i64::try_from(value).unwrap_or(i64::MAX)
}

pub fn serialize_json<T: Serialize>(value: &T, label: &str) -> Result<JsonValue> {
    serde_json::to_value(value).with_context(|| format!("failed to serialize {label}"))
}

pub fn parse_args_json(args: &[u8]) -> JsonValue {
    if args.is_empty() {
        return JsonValue::Null;
    }

    serde_json::from_slice::<JsonValue>(args)
        .or_else(|_| {
            String::from_utf8(args.to_vec())
                .ok()
                .and_then(|text| serde_json::from_str::<JsonValue>(&text).ok())
                .ok_or_else(|| serde_json::Error::io(std::io::Error::other("invalid json")))
        })
        .unwrap_or_else(|_| json!({ "value": "invalid json" }))
}

pub fn action_kind_from_json(action_json: &JsonValue) -> String {
    match action_json {
        JsonValue::Object(map) => map
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "Unknown".to_string()),
        JsonValue::String(value) => value.clone(),
        _ => "Unknown".to_string(),
    }
}

pub fn extract_status(status: &ExecutionStatusView) -> (&'static str, Option<JsonValue>) {
    match status {
        ExecutionStatusView::Unknown => (
            "Unknown",
            Some(json!({
                "status_type": "Unknown",
            })),
        ),
        ExecutionStatusView::Failure(error) => (
            "Failure",
            Some(json!({
                "error": error.to_string(),
                "status_type": "Failure",
            })),
        ),
        ExecutionStatusView::SuccessReceiptId(receipt_id) => (
            "SuccessReceiptId",
            Some(json!({
                "receipt_id": receipt_id.to_string(),
                "status_type": "SuccessReceiptId",
            })),
        ),
        ExecutionStatusView::SuccessValue(raw_value) => {
            if raw_value.is_empty() {
                return ("SuccessValue", None);
            }

            let parsed = serde_json::from_slice::<JsonValue>(raw_value)
                .or_else(|_| {
                    String::from_utf8(raw_value.clone())
                        .ok()
                        .and_then(|text| serde_json::from_str::<JsonValue>(&text).ok())
                        .ok_or_else(|| serde_json::Error::io(std::io::Error::other("invalid json")))
                })
                .unwrap_or_else(|_| json!({ "value": "invalid json" }));

            ("SuccessValue", Some(parsed))
        }
    }
}

pub fn block_to_row(block: &BlockView, author: &str) -> Result<NearBlockRow> {
    let header = &block.header;

    Ok(NearBlockRow {
        height: header.height as i64,
        hash: header.hash.to_string(),
        prev_hash: header.prev_hash.to_string(),
        epoch_id: header.epoch_id.to_string(),
        next_epoch_id: header.next_epoch_id.to_string(),
        author: author.to_string(),
        timestamp: header.timestamp as i64,
        timestamp_nanosec: header.timestamp_nanosec as i64,
        gas_price: header.gas_price.to_string(),
        total_supply: header.total_supply.to_string(),
        validator_reward: header.validator_reward.to_string(),
        protocol_version: header.latest_protocol_version as i32,
        chunks_included: header.chunks_included as i64,
        block_ordinal: header.block_ordinal.map(|value| value as i64),
        prev_height: header.prev_height.map(|value| value as i64),
        raw_data: serialize_json(block, "block raw_data")?,
    })
}

pub fn chunk_to_row(
    chunk: &IndexerChunkView,
    block_height: i64,
    shard_id: i64,
) -> Result<NearChunkRow> {
    let header = &chunk.header;

    Ok(NearChunkRow {
        chunk_hash: header.chunk_hash.to_string(),
        block_height,
        shard_id,
        author: chunk.author.to_string(),
        height_created: header.height_created as i64,
        height_included: header.height_included as i64,
        gas_used: header.gas_used.as_gas() as i64,
        gas_limit: header.gas_limit.as_gas() as i64,
        validator_reward: header.validator_reward.to_string(),
        raw_data: serialize_json(chunk, "chunk raw_data")?,
    })
}

pub fn state_changes_to_row(
    state_changes: &StateChangesView,
    block_height: i64,
    shard_id: i64,
) -> Result<NearStateChangesRow> {
    Ok(NearStateChangesRow {
        block_height,
        shard_id,
        state_changes: serialize_json(state_changes, "state changes payload")?,
    })
}

pub fn transaction_to_row(
    transaction: &near_lake_framework::near_indexer_primitives::views::SignedTransactionView,
    block_height: i64,
    shard_id: i64,
) -> Result<NearTransactionRow> {
    Ok(NearTransactionRow {
        hash: transaction.hash.to_string(),
        block_height,
        shard_id,
        signer_id: transaction.signer_id.to_string(),
        receiver_id: transaction.receiver_id.to_string(),
        nonce: transaction.nonce as i64,
        priority_fee: transaction.priority_fee as i64,
        public_key: transaction.public_key.to_string(),
        signature: transaction.signature.to_string(),
        actions: serialize_json(&transaction.actions, "transaction actions")?,
        raw_data: serialize_json(transaction, "transaction raw_data")?,
    })
}

pub fn receipt_to_row(
    receipt: &ReceiptView,
    block_height: i64,
    shard_id: i64,
) -> Result<NearReceiptRow> {
    Ok(NearReceiptRow {
        receipt_id: receipt.receipt_id.to_string(),
        block_height,
        shard_id,
        predecessor_id: receipt.predecessor_id.to_string(),
        receiver_id: receipt.receiver_id.to_string(),
        receipt: serialize_json(&receipt.receipt, "receipt body")?,
        raw_data: serialize_json(receipt, "receipt row")?,
    })
}

pub fn execution_outcome_to_row(
    outcome_view: &ExecutionOutcomeWithIdView,
    meta: ExecutionOutcomeRowMeta<'_>,
) -> Result<NearExecutionOutcomeRow> {
    let outcome = &outcome_view.outcome;
    let outcome_receipt_ids = JsonValue::Array(
        outcome
            .receipt_ids
            .iter()
            .map(|value| JsonValue::String(value.to_string()))
            .collect(),
    );
    let logs = JsonValue::Array(
        outcome
            .logs
            .iter()
            .cloned()
            .map(JsonValue::String)
            .collect(),
    );
    let (status, results_json) = extract_status(&outcome.status);

    Ok(NearExecutionOutcomeRow {
        id: outcome_view.id.to_string(),
        transaction_hash: meta.transaction_hash.map(ToString::to_string),
        block_height: meta.block_height,
        shard_id: meta.shard_id,
        receipt_id: meta.receipt_id.map(ToString::to_string),
        outcome: serialize_json(&outcome_view.outcome, "execution outcome")?,
        raw_data: serialize_json(outcome_view, "execution outcome row")?,
        block_hash: meta.block_hash.to_string(),
        chunk_hash: meta.chunk_hash.to_string(),
        gas_burnt: outcome.gas_burnt.as_gas() as i64,
        gas_used: outcome.gas_burnt.as_gas() as f64,
        tokens_burnt: outcome.tokens_burnt.as_yoctonear() as f64 / 1e24,
        executor_account_id: outcome.executor_id.to_string(),
        status: status.to_string(),
        outcome_receipt_ids,
        executed_in_block_hash: meta.block_hash.to_string(),
        logs,
        results_json,
        block_timestamp: meta.block_timestamp,
    })
}

pub fn receipt_action_rows(
    receipt: &ReceiptView,
    meta: ReceiptActionRowMeta<'_>,
) -> Result<Vec<NearReceiptActionRow>> {
    let near_lake_framework::near_indexer_primitives::views::ReceiptEnumView::Action {
        signer_id,
        signer_public_key,
        gas_price,
        actions,
        ..
    } = &receipt.receipt
    else {
        return Ok(Vec::new());
    };

    let mut rows = Vec::with_capacity(actions.len());
    for (action_index, action) in actions.iter().enumerate() {
        let action_json = serialize_json(action, "receipt action")?;
        let action_kind = action_kind_from_json(&action_json);
        let signer_id_value = signer_id.to_string();
        let signer_public_key_value = signer_public_key.to_string();
        let gas_price_value = gas_price.to_string();

        let mut method_name: Option<String> = None;
        let mut gas: Option<i64> = None;
        let mut deposit: Option<String> = None;
        let mut args_base64: Option<String> = None;
        let mut args_json: Option<JsonValue> = None;

        if let ActionView::FunctionCall {
            method_name: function_name,
            args,
            gas: function_gas,
            deposit: function_deposit,
        } = action
        {
            let args_bytes: &[u8] = args.as_ref();
            method_name = Some(function_name.clone());
            gas = Some(function_gas.as_gas() as i64);
            deposit = Some(function_deposit.to_string());
            args_base64 = Some(BASE64_STANDARD.encode(args_bytes));
            args_json = Some(parse_args_json(args_bytes));
        }

        rows.push(NearReceiptActionRow {
            id: format!("{}-{action_index}", receipt.receipt_id),
            transaction_hash: meta.transaction_hash.map(ToString::to_string),
            block_height: meta.block_height,
            receipt_id: receipt.receipt_id.to_string(),
            execution_outcome_id: meta.execution_outcome_id.to_string(),
            shard_id: meta.shard_id,
            signer_account_id: Some(signer_id_value),
            signer_public_key: Some(signer_public_key_value),
            gas_price: Some(gas_price_value),
            action_kind,
            action_json,
            predecessor_id: receipt.predecessor_id.to_string(),
            receiver_id: receipt.receiver_id.to_string(),
            block_hash: meta.block_hash.to_string(),
            chunk_hash: meta.chunk_hash.to_string(),
            author: meta.author.to_string(),
            method_name,
            gas,
            deposit,
            args_base64,
            args_json,
            action_index: action_index as i32,
            block_timestamp: meta.block_timestamp,
        });
    }

    Ok(rows)
}
