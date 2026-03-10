use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value as JsonValue};
use std::collections::HashMap;

use super::sql::{
    amount_from_optional_raw, chain_ref, config_field_from_method, decode_cursor, encode_cursor,
    is_success_status, json_value_to_i64, json_value_to_string, json_value_to_string_vec,
    load_canonical_actions, normalized_contract_id_filter, normalized_text_filter,
    parse_config_value, require_db, resolve_limit, timestamp_from_ns_opt,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffsetCursor {
    offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigHistoryProjection {
    pub contract_id: String,
    pub field: String,
    pub method_name: String,
    pub actor_account_id: String,
    pub value: JsonValue,
    pub refs: VenearChainRefDto,
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigSnapshotProjection {
    pub contract_id: String,
    pub owner_account_id: Option<String>,
    pub proposed_new_owner_account_id: Option<String>,
    pub guardians: Vec<String>,
    pub reviewer_ids: Vec<String>,
    pub lockup_contract_hash: Option<String>,
    pub lockup_code_deployers: Vec<String>,
    pub unlock_duration_sec: Option<u64>,
    pub min_lockup_deposit_raw: Option<String>,
    pub local_deposit_raw: Option<String>,
    pub staking_pool_whitelist_account_id: Option<String>,
    pub paused: Option<bool>,
    pub refs: Option<VenearChainRefDto>,
    pub extra_fields: JsonValue,
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/config",
    tag = "venear",
    operation_id = "getVenearConfigCanonical",
    responses(
        (status = 200, description = "Current veNEAR config snapshots derived from canonical NEAR actions", body = VenearConfigResponse),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_config(
    State(app_state): State<AppState>,
) -> Result<Json<VenearConfigResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let mut snapshots = load_config_snapshots(db.as_ref()).await?;
    snapshots.sort_by(|left, right| left.contract_id.cmp(&right.contract_id));

    Ok(Json(VenearConfigResponse {
        data: snapshots
            .into_iter()
            .map(|snapshot| VenearConfigSnapshotDto {
                contract_id: snapshot.contract_id,
                owner_account_id: snapshot.owner_account_id,
                proposed_new_owner_account_id: snapshot.proposed_new_owner_account_id,
                guardians: snapshot.guardians,
                reviewer_ids: snapshot.reviewer_ids,
                lockup_contract_hash: snapshot.lockup_contract_hash,
                lockup_code_deployers: snapshot.lockup_code_deployers,
                unlock_duration_sec: snapshot.unlock_duration_sec,
                min_lockup_deposit: amount_from_optional_raw(snapshot.min_lockup_deposit_raw),
                local_deposit: amount_from_optional_raw(snapshot.local_deposit_raw),
                staking_pool_whitelist_account_id: snapshot.staking_pool_whitelist_account_id,
                paused: snapshot.paused,
                refs: snapshot.refs,
                extra_fields: snapshot.extra_fields,
            })
            .collect(),
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/config/history",
    params(VenearConfigHistoryQueryV2),
    tag = "venear",
    operation_id = "listVenearConfigHistoryCanonical",
    responses(
        (status = 200, description = "Normalized veNEAR config changes derived from canonical NEAR actions", body = VenearConfigHistoryListResponseV2),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_config_history(
    State(app_state): State<AppState>,
    Query(query): Query<VenearConfigHistoryQueryV2>,
) -> Result<Json<VenearConfigHistoryListResponseV2>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let contract_filter = normalized_contract_id_filter(query.contract_id.as_deref());
    let field_filter = normalized_text_filter(query.field.as_deref());
    let method_filter = normalized_text_filter(query.method_name.as_deref());

    let mut history = load_config_history(db.as_ref()).await?;
    history.retain(|entry| {
        if let Some(contract_id) = contract_filter.as_deref() {
            if entry.contract_id != contract_id {
                return false;
            }
        }
        if let Some(field) = field_filter.as_deref() {
            if entry.field != field {
                return false;
            }
        }
        if let Some(method_name) = method_filter.as_deref() {
            if entry.method_name != method_name {
                return false;
            }
        }
        true
    });
    history.sort_by(|left, right| {
        right
            .refs
            .block_height
            .cmp(&left.refs.block_height)
            .then_with(|| right.refs.receipt_id.cmp(&left.refs.receipt_id))
    });

    let total = history.len();
    let data = history
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .map(|entry| VenearConfigHistoryEntryV2Dto {
            contract_id: entry.contract_id,
            field: entry.field,
            method_name: entry.method_name,
            actor_account_id: entry.actor_account_id,
            value: entry.value,
            refs: entry.refs,
        })
        .collect::<Vec<_>>();

    paginate_history(data, cursor.offset, limit, total).map(Json)
}

pub(crate) async fn load_config_history(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<ConfigHistoryProjection>, ApiErrorResponse> {
    let actions = load_canonical_actions(
        db,
        &[
            "new",
            "set_guardians",
            "set_reviewer_ids",
            "set_lockup_contract",
            "set_lockup_code_deployers",
            "set_local_deposit",
            "set_staking_pool_whitelist_account_id",
            "set_unlock_duration_sec",
            "propose_new_owner_account_id",
            "accept_ownership",
            "pause",
            "unpause",
        ],
    )
    .await?;

    let mut history = Vec::new();
    for action in actions
        .into_iter()
        .filter(|action| is_success_status(action.status.as_deref()))
    {
        let fields = if action.method_name == "new" {
            action
                .args_json
                .get("config")
                .and_then(|value| value.as_object())
                .map(|object| object.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        } else {
            config_field_from_method(&action.method_name)
                .into_iter()
                .map(|field| field.to_string())
                .collect::<Vec<_>>()
        };

        for field in fields {
            let value = if action.method_name == "accept_ownership" && field == "owner_account_id" {
                JsonValue::String(action.predecessor_id.clone())
            } else {
                parse_config_value(&action.method_name, &field, &action.args_json)
            };
            history.push(ConfigHistoryProjection {
                contract_id: action.contract_id.clone(),
                field,
                method_name: action.method_name.clone(),
                actor_account_id: action.predecessor_id.clone(),
                value,
                refs: chain_ref(
                    action.contract_id.clone(),
                    action.transaction_hash.clone(),
                    action.receipt_id.clone(),
                    action.execution_outcome_id.clone(),
                    action.block_height,
                    timestamp_from_ns_opt(Some(action.block_timestamp))
                        .expect("timestamp conversion")
                        .unwrap_or_default(),
                ),
            });
        }
    }

    Ok(history)
}

pub(crate) async fn load_config_snapshots(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<ConfigSnapshotProjection>, ApiErrorResponse> {
    let history = load_config_history(db).await?;
    let mut snapshots = HashMap::<String, ConfigSnapshotProjection>::new();

    for entry in history {
        let snapshot = snapshots
            .entry(entry.contract_id.clone())
            .or_insert_with(|| ConfigSnapshotProjection {
                contract_id: entry.contract_id.clone(),
                owner_account_id: None,
                proposed_new_owner_account_id: None,
                guardians: Vec::new(),
                reviewer_ids: Vec::new(),
                lockup_contract_hash: None,
                lockup_code_deployers: Vec::new(),
                unlock_duration_sec: None,
                min_lockup_deposit_raw: None,
                local_deposit_raw: None,
                staking_pool_whitelist_account_id: None,
                paused: None,
                refs: None,
                extra_fields: JsonValue::Object(Map::new()),
            });

        match entry.field.as_str() {
            "owner_account_id" => {
                snapshot.owner_account_id = json_value_to_string(&entry.value);
            }
            "proposed_new_owner_account_id" => {
                snapshot.proposed_new_owner_account_id = json_value_to_string(&entry.value);
            }
            "guardians" => {
                snapshot.guardians = json_value_to_string_vec(&entry.value);
            }
            "reviewer_ids" => {
                snapshot.reviewer_ids = json_value_to_string_vec(&entry.value);
            }
            "lockup_contract_hash" => {
                snapshot.lockup_contract_hash = json_value_to_string(&entry.value);
            }
            "lockup_code_deployers" => {
                snapshot.lockup_code_deployers = json_value_to_string_vec(&entry.value);
            }
            "unlock_duration_sec" => {
                snapshot.unlock_duration_sec =
                    json_value_to_i64(&entry.value).and_then(|value| u64::try_from(value).ok());
            }
            "unlock_duration_ns" => {
                snapshot.unlock_duration_sec = json_value_to_i64(&entry.value)
                    .map(|value| value / 1_000_000_000)
                    .and_then(|value| u64::try_from(value).ok());
            }
            "min_lockup_deposit" => {
                snapshot.min_lockup_deposit_raw = json_value_to_string(&entry.value);
            }
            "local_deposit" => {
                snapshot.local_deposit_raw = json_value_to_string(&entry.value);
            }
            "staking_pool_whitelist_account_id" => {
                snapshot.staking_pool_whitelist_account_id = json_value_to_string(&entry.value);
            }
            "paused" => {
                snapshot.paused = entry.value.as_bool();
            }
            _ => {
                let object = snapshot.extra_fields.as_object_mut().expect("json object");
                object.insert(entry.field.clone(), entry.value.clone());
            }
        }

        snapshot.refs = Some(entry.refs);
    }

    Ok(snapshots.into_values().collect())
}

fn paginate_history(
    mut data: Vec<VenearConfigHistoryEntryV2Dto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearConfigHistoryListResponseV2, ApiErrorResponse> {
    let has_more = data.len() > limit as usize;
    if has_more {
        data.pop();
    }
    let next_cursor = if has_more && offset + data.len() < total {
        Some(encode_cursor(&OffsetCursor {
            offset: offset + data.len(),
        })?)
    } else {
        None
    };
    Ok(VenearConfigHistoryListResponseV2 {
        data,
        next_cursor,
        has_more,
    })
}
