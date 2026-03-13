use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, Value,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(crate) struct ParsedEventLog {
    pub event_type: String,
    pub payload: JsonValue,
    pub log_index: i64,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct CanonicalAction {
    pub id: String,
    pub contract_id: String,
    pub predecessor_id: String,
    pub signer_account_id: Option<String>,
    pub method_name: String,
    pub args_json: JsonValue,
    pub status: Option<String>,
    pub logs: Vec<ParsedEventLog>,
    pub results_json: Option<JsonValue>,
    pub block_height: i64,
    pub block_timestamp: i64,
    pub transaction_hash: Option<String>,
    pub receipt_id: String,
    pub execution_outcome_id: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ReceiptEdge {
    pub parent_receipt_id: String,
    pub child_receipt_id: String,
}

pub(crate) fn require_db(
    app_state: &AppState,
) -> Result<Arc<DatabaseConnection>, ApiErrorResponse> {
    app_state
        .db
        .clone()
        .ok_or_else(|| ApiErrorResponse::service_unavailable("Database connection unavailable"))
}

pub(crate) fn resolve_limit(limit: Option<u64>) -> u64 {
    limit.unwrap_or(20).clamp(1, 100)
}

pub(crate) fn normalized_text_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

pub(crate) fn normalized_contract_id_filter(value: Option<&str>) -> Option<String> {
    normalized_text_filter(value).map(|value| value.to_ascii_lowercase())
}

pub(crate) fn normalized_event_type_filter(value: Option<&str>) -> Option<String> {
    normalized_text_filter(value).map(|value| value.to_ascii_lowercase())
}

pub(crate) fn parse_direction_filter(
    value: Option<&str>,
) -> Result<Option<String>, ApiErrorResponse> {
    let Some(direction) = normalized_text_filter(value) else {
        return Ok(None);
    };

    match direction.to_ascii_lowercase().as_str() {
        "inbound" => Ok(Some("inbound".to_string())),
        "outbound" => Ok(Some("outbound".to_string())),
        _ => Err(ApiErrorResponse::bad_request(
            "Invalid direction filter. Use 'inbound' or 'outbound'.",
        )),
    }
}

pub(crate) fn parse_rfc3339_to_ns(value: Option<&str>) -> Result<Option<i64>, ApiErrorResponse> {
    let Some(value) = normalized_text_filter(value) else {
        return Ok(None);
    };

    let parsed = DateTime::parse_from_rfc3339(&value).map_err(|err| {
        ApiErrorResponse::bad_request("Invalid RFC 3339 timestamp filter")
            .with_detail(err.to_string())
    })?;

    Ok(parsed.timestamp_nanos_opt())
}

pub(crate) fn ns_to_rfc3339(epoch_nanoseconds: i64) -> Result<String, ApiErrorResponse> {
    let seconds = epoch_nanoseconds.div_euclid(1_000_000_000);
    let nanos = epoch_nanoseconds.rem_euclid(1_000_000_000) as u32;

    DateTime::<Utc>::from_timestamp(seconds, nanos)
        .map(|value| value.to_rfc3339())
        .ok_or_else(|| {
            ApiErrorResponse::internal_server_error("Invalid epoch nanoseconds in stored data")
        })
}

pub(crate) fn timestamp_from_ns_opt(
    value: Option<i64>,
) -> Result<Option<String>, ApiErrorResponse> {
    value.map(ns_to_rfc3339).transpose()
}

pub(crate) fn decode_row_col<T: sea_orm::TryGetable>(
    row: &QueryResult,
    column: &str,
    entity_label: &str,
) -> Result<T, ApiErrorResponse> {
    row.try_get("", column).map_err(|err| {
        ApiErrorResponse::internal_server_error(format!(
            "Failed to decode {entity_label} row column '{column}'"
        ))
        .with_detail(err.to_string())
    })
}

#[allow(dead_code)]
pub(crate) async fn scalar_i64_with_values(
    db: &DatabaseConnection,
    sql: &str,
    values: Vec<Value>,
) -> Result<i64, ApiErrorResponse> {
    let result = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            values,
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let Some(result) = result else {
        return Ok(0);
    };

    result.try_get("", "value").map_err(|err| {
        ApiErrorResponse::internal_server_error("Failed to decode scalar query result")
            .with_detail(err.to_string())
    })
}

pub(crate) fn encode_cursor<T: Serialize>(payload: &T) -> Result<String, ApiErrorResponse> {
    let bytes = serde_json::to_vec(payload).map_err(|err| {
        ApiErrorResponse::internal_server_error("Failed to encode pagination cursor")
            .with_detail(err.to_string())
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) fn decode_cursor<T: DeserializeOwned>(cursor: &str) -> Result<T, ApiErrorResponse> {
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|err| {
        ApiErrorResponse::bad_request("Invalid cursor value").with_detail(err.to_string())
    })?;

    serde_json::from_slice::<T>(&bytes).map_err(|err| {
        ApiErrorResponse::bad_request("Invalid cursor payload").with_detail(err.to_string())
    })
}

pub(crate) fn json_value_to_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(text) => Some(text.clone()),
        JsonValue::Number(number) => Some(number.to_string()),
        JsonValue::Bool(flag) => Some(flag.to_string()),
        _ => None,
    }
}

pub(crate) fn json_value_to_i64(value: &JsonValue) -> Option<i64> {
    match value {
        JsonValue::Number(number) => number.as_i64(),
        JsonValue::String(text) => text.parse::<i64>().ok(),
        _ => None,
    }
}

pub(crate) fn json_value_to_string_vec(value: &JsonValue) -> Vec<String> {
    let Some(array) = value.as_array() else {
        return Vec::new();
    };

    array
        .iter()
        .filter_map(json_value_to_string)
        .filter(|entry| !entry.is_empty())
        .collect()
}

pub(crate) fn amount_from_raw(raw: impl Into<String>) -> VenearTokenAmountDto {
    let raw = raw.into();
    VenearTokenAmountDto {
        decimal: raw_to_decimal(&raw, 24),
        raw,
        symbol: "veNEAR".to_string(),
        decimals: 24,
    }
}

pub(crate) fn amount_from_optional_raw(raw: Option<String>) -> Option<VenearTokenAmountDto> {
    raw.map(amount_from_raw)
}

pub(crate) fn raw_to_decimal(raw: &str, decimals: usize) -> String {
    let mut digits = raw.trim();
    if digits.is_empty() {
        return "0".to_string();
    }

    let negative = digits.starts_with('-');
    if negative {
        digits = &digits[1..];
    }

    let digits = digits.trim_start_matches('0');
    if digits.is_empty() {
        return "0".to_string();
    }

    let rendered = if digits.len() <= decimals {
        let fractional = format!("{:0>width$}", digits, width = decimals);
        let fractional = fractional.trim_end_matches('0');
        if fractional.is_empty() {
            "0".to_string()
        } else {
            format!("0.{fractional}")
        }
    } else {
        let split = digits.len() - decimals;
        let integer = &digits[..split];
        let fractional = digits[split..].trim_end_matches('0');
        if fractional.is_empty() {
            integer.to_string()
        } else {
            format!("{integer}.{fractional}")
        }
    };

    if negative && rendered != "0" {
        format!("-{rendered}")
    } else {
        rendered
    }
}

pub(crate) fn chain_ref(
    contract_id: String,
    transaction_hash: Option<String>,
    receipt_id: String,
    execution_outcome_id: String,
    block_height: i64,
    block_timestamp: String,
) -> VenearChainRefDto {
    VenearChainRefDto {
        contract_id,
        transaction_hash,
        receipt_id,
        execution_outcome_id,
        block_height,
        block_timestamp,
    }
}

pub(crate) fn derive_lockup_account_id(owner_account_id: &str, contract_id: &str) -> String {
    let digest = Sha256::digest(owner_account_id.as_bytes());
    format!("{}.{}", hex::encode(&digest[..20]), contract_id)
}

pub(crate) async fn load_canonical_actions(
    db: &DatabaseConnection,
    method_names: &[&str],
) -> Result<Vec<CanonicalAction>, ApiErrorResponse> {
    let mut conditions = String::new();
    if !method_names.is_empty() {
        let quoted = method_names
            .iter()
            .map(|name| format!("'{}'", name.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(", ");
        conditions = format!(" AND ra.method_name IN ({quoted})");
    }

    let sql = format!(
        "
        SELECT
            ra.id,
            ra.receiver_id AS contract_id,
            ra.predecessor_id,
            ra.signer_account_id,
            ra.method_name,
            ra.args_json,
            eo.status,
            eo.logs,
            eo.results_json,
            ra.block_height,
            ra.block_timestamp,
            ra.transaction_hash,
            ra.receipt_id,
            ra.execution_outcome_id
        FROM near.near_receipt_actions ra
        LEFT JOIN near.near_execution_outcomes eo
            ON eo.receipt_id = ra.receipt_id
           AND near.is_hos_contract(eo.executor_account_id) = TRUE
        WHERE ra.action_kind = 'FunctionCall'
          AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
          {conditions}
        ORDER BY ra.block_timestamp ASC, ra.receipt_id ASC, ra.action_index ASC
        "
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    rows.iter()
        .map(|row| {
            let logs = decode_row_col::<Option<JsonValue>>(row, "logs", "canonical action")?;
            Ok(CanonicalAction {
                id: decode_row_col::<String>(row, "id", "canonical action")?,
                contract_id: decode_row_col::<String>(row, "contract_id", "canonical action")?,
                predecessor_id: decode_row_col::<String>(
                    row,
                    "predecessor_id",
                    "canonical action",
                )?,
                signer_account_id: decode_row_col::<Option<String>>(
                    row,
                    "signer_account_id",
                    "canonical action",
                )?,
                method_name: decode_row_col::<String>(row, "method_name", "canonical action")?,
                args_json: decode_row_col::<Option<JsonValue>>(
                    row,
                    "args_json",
                    "canonical action",
                )?
                .unwrap_or(JsonValue::Null),
                status: decode_row_col::<Option<String>>(row, "status", "canonical action")?,
                logs: parse_event_logs(logs),
                results_json: decode_row_col::<Option<JsonValue>>(
                    row,
                    "results_json",
                    "canonical action",
                )?,
                block_height: decode_row_col::<i64>(row, "block_height", "canonical action")?,
                block_timestamp: decode_row_col::<i64>(row, "block_timestamp", "canonical action")?,
                transaction_hash: decode_row_col::<Option<String>>(
                    row,
                    "transaction_hash",
                    "canonical action",
                )?,
                receipt_id: decode_row_col::<String>(row, "receipt_id", "canonical action")?,
                execution_outcome_id: decode_row_col::<String>(
                    row,
                    "execution_outcome_id",
                    "canonical action",
                )?,
            })
        })
        .collect()
}

pub(crate) async fn load_receipt_edges(
    db: &DatabaseConnection,
    receipt_ids: &[String],
) -> Result<Vec<ReceiptEdge>, ApiErrorResponse> {
    if receipt_ids.is_empty() {
        return Ok(Vec::new());
    }

    let quoted = receipt_ids
        .iter()
        .map(|id| format!("'{}'", id.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");

    let sql = format!(
        "
        SELECT parent_receipt_id, child_receipt_id
        FROM near.near_receipt_edges
        WHERE parent_receipt_id IN ({quoted})
           OR child_receipt_id IN ({quoted})
        "
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    rows.iter()
        .map(|row| {
            Ok(ReceiptEdge {
                parent_receipt_id: decode_row_col::<String>(
                    row,
                    "parent_receipt_id",
                    "receipt edge",
                )?,
                child_receipt_id: decode_row_col::<String>(
                    row,
                    "child_receipt_id",
                    "receipt edge",
                )?,
            })
        })
        .collect()
}

pub(crate) fn is_success_status(status: Option<&str>) -> bool {
    matches!(status, Some("SuccessValue" | "SuccessReceiptId"))
}

pub(crate) fn parse_event_logs(logs: Option<JsonValue>) -> Vec<ParsedEventLog> {
    let Some(logs) = logs.and_then(|value| value.as_array().cloned()) else {
        return Vec::new();
    };

    logs.into_iter()
        .enumerate()
        .filter_map(|(index, value)| match value {
            JsonValue::String(line) if line.starts_with("EVENT_JSON:") => {
                let payload =
                    serde_json::from_str::<JsonValue>(&line["EVENT_JSON:".len()..]).ok()?;
                let event_type = payload.get("event").and_then(json_value_to_string)?;
                Some(ParsedEventLog {
                    event_type,
                    payload,
                    log_index: index as i64,
                })
            }
            _ => None,
        })
        .collect()
}

pub(crate) fn compute_proposal_status(
    rejected_at_ns: Option<i64>,
    voting_start_at_ns: Option<i64>,
    voting_duration_ns: Option<i64>,
    finalized: bool,
) -> String {
    if rejected_at_ns.is_some() {
        return "rejected".to_string();
    }

    if !finalized {
        return "created".to_string();
    }

    let Some(voting_start_at_ns) = voting_start_at_ns else {
        return "approved".to_string();
    };

    let now_ns = Utc::now().timestamp_micros().saturating_mul(1_000);
    if now_ns < voting_start_at_ns {
        return "approved".to_string();
    }

    let Some(voting_duration_ns) = voting_duration_ns else {
        return "voting".to_string();
    };

    if now_ns >= voting_start_at_ns.saturating_add(voting_duration_ns) {
        "finished".to_string()
    } else {
        "voting".to_string()
    }
}

pub(crate) fn config_field_from_method(method_name: &str) -> Vec<&'static str> {
    match method_name {
        "set_guardians" => vec!["guardians"],
        "set_reviewer_ids" => vec!["reviewer_ids"],
        "set_lockup_contract" => vec!["lockup_contract_hash", "min_lockup_deposit"],
        "set_lockup_code_deployers" => vec!["lockup_code_deployers"],
        "set_local_deposit" => vec!["local_deposit"],
        "set_staking_pool_whitelist_account_id" => vec!["staking_pool_whitelist_account_id"],
        "set_unlock_duration_sec" => vec!["unlock_duration_sec"],
        "propose_new_owner_account_id" => vec!["proposed_new_owner_account_id"],
        "accept_ownership" => vec!["owner_account_id"],
        "pause" | "unpause" => vec!["paused"],
        _ => Vec::new(),
    }
}

pub(crate) fn parse_config_value(
    method_name: &str,
    field: &str,
    args_json: &JsonValue,
) -> JsonValue {
    match (method_name, field) {
        ("new", _) => args_json
            .get("config")
            .and_then(|config| config.get(field))
            .cloned()
            .unwrap_or(JsonValue::Null),
        ("set_guardians", "guardians") => args_json
            .get("guardians")
            .cloned()
            .unwrap_or(JsonValue::Null),
        ("set_reviewer_ids", "reviewer_ids") => args_json
            .get("reviewer_ids")
            .cloned()
            .unwrap_or(JsonValue::Null),
        ("set_lockup_contract", "lockup_contract_hash") => args_json
            .get("contract_hash")
            .cloned()
            .or_else(|| args_json.get("contractHash").cloned())
            .unwrap_or(JsonValue::Null),
        ("set_lockup_contract", "min_lockup_deposit") => args_json
            .get("min_lockup_deposit")
            .cloned()
            .unwrap_or(JsonValue::Null),
        ("set_lockup_code_deployers", "lockup_code_deployers") => args_json
            .get("lockup_code_deployers")
            .cloned()
            .unwrap_or(JsonValue::Null),
        ("set_local_deposit", "local_deposit") => args_json
            .get("local_deposit")
            .cloned()
            .unwrap_or(JsonValue::Null),
        ("set_staking_pool_whitelist_account_id", "staking_pool_whitelist_account_id") => args_json
            .get("staking_pool_whitelist_account_id")
            .cloned()
            .unwrap_or(JsonValue::Null),
        ("set_unlock_duration_sec", "unlock_duration_sec") => args_json
            .get("unlock_duration_sec")
            .cloned()
            .unwrap_or(JsonValue::Null),
        ("propose_new_owner_account_id", "proposed_new_owner_account_id") => args_json
            .get("new_owner_account_id")
            .cloned()
            .or_else(|| args_json.get("proposed_new_owner_account_id").cloned())
            .unwrap_or(JsonValue::Null),
        ("accept_ownership", "owner_account_id") => JsonValue::Null,
        ("pause", "paused") => JsonValue::Bool(true),
        ("unpause", "paused") => JsonValue::Bool(false),
        _ => JsonValue::Null,
    }
}
