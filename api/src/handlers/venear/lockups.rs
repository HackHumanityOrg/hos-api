use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

use super::{
    accounts::ensure_account_exists,
    delegations::load_active_delegations,
    sql::{
        amount_from_optional_raw, amount_from_raw, chain_ref, decode_cursor,
        derive_lockup_account_id, encode_cursor, is_success_status, json_value_to_string,
        load_canonical_actions, normalized_text_filter, require_db, resolve_limit,
        timestamp_from_ns_opt,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffsetCursor {
    offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct LockupEventProjection {
    pub lockup_account_id: String,
    pub owner_account_id: String,
    pub contract_id: String,
    pub event_type: String,
    pub locked_near_balance_raw: Option<String>,
    pub block_timestamp: i64,
    pub refs: VenearChainRefDto,
    pub payload: JsonValue,
}

#[derive(Debug, Clone)]
pub(crate) struct LockupProjection {
    pub lockup_account_id: String,
    pub owner_account_id: String,
    pub contract_id: String,
    pub status: String,
    pub locked_near_balance_raw: String,
    pub delegated_to_account_id: Option<String>,
    pub deployment_refs: Option<VenearChainRefDto>,
    pub latest_refs: VenearChainRefDto,
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/lockups",
    params(VenearLockupsQuery),
    tag = "venear",
    operation_id = "listVenearLockupsCanonical",
    responses(
        (status = 200, description = "Current lockup summaries derived from canonical NEAR actions", body = VenearLockupListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_lockups(
    State(app_state): State<AppState>,
    Query(query): Query<VenearLockupsQuery>,
) -> Result<Json<VenearLockupListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let owner_filter = normalized_text_filter(query.owner_account_id.as_deref());
    let status_filter =
        normalized_text_filter(query.status.as_deref()).map(|value| value.to_ascii_lowercase());

    let mut lockups = load_lockup_projections(db.as_ref()).await?;
    lockups.retain(|lockup| {
        if let Some(owner) = owner_filter.as_deref() {
            if lockup.owner_account_id != owner {
                return false;
            }
        }
        if let Some(status) = status_filter.as_deref() {
            if lockup.status != status {
                return false;
            }
        }
        if let Some(has_delegatee) = query.has_delegatee {
            if lockup.delegated_to_account_id.is_some() != has_delegatee {
                return false;
            }
        }
        true
    });
    lockups.sort_by(|left, right| left.owner_account_id.cmp(&right.owner_account_id));

    let total = lockups.len();
    let data = lockups
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .map(|lockup| VenearLockupSummaryDto {
            lockup_account_id: lockup.lockup_account_id,
            owner_account_id: lockup.owner_account_id,
            contract_id: lockup.contract_id,
            status: lockup.status,
            locked_near_balance: amount_from_raw(lockup.locked_near_balance_raw),
            delegated_to_account_id: lockup.delegated_to_account_id,
            last_updated_at: lockup.latest_refs.block_timestamp.clone(),
            refs: lockup.latest_refs,
        })
        .collect::<Vec<_>>();

    paginate_lockups(data, cursor.offset, limit, total).map(Json)
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/lockups/{lockup_account_id}",
    tag = "venear",
    operation_id = "getVenearLockupCanonical",
    responses(
        (status = 200, description = "Current lockup detail derived from canonical NEAR actions", body = VenearLockupDetailDto),
        (status = 404, description = "Lockup not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_lockup(
    State(app_state): State<AppState>,
    Path(lockup_account_id): Path<String>,
) -> Result<Json<VenearLockupDetailDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let lockup = load_lockup_projections(db.as_ref())
        .await?
        .into_iter()
        .find(|lockup| lockup.lockup_account_id == lockup_account_id)
        .ok_or_else(|| ApiErrorResponse::not_found("Lockup not found"))?;

    Ok(Json(VenearLockupDetailDto {
        lockup_account_id: lockup.lockup_account_id,
        owner_account_id: lockup.owner_account_id,
        contract_id: lockup.contract_id,
        status: lockup.status,
        locked_near_balance: amount_from_raw(lockup.locked_near_balance_raw),
        delegated_to_account_id: lockup.delegated_to_account_id,
        deployment_refs: lockup.deployment_refs,
        latest_refs: lockup.latest_refs,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/lockups/{lockup_account_id}/events",
    params(VenearLockupEventsQuery),
    tag = "venear",
    operation_id = "listVenearLockupEventsCanonical",
    responses(
        (status = 200, description = "Lockup lifecycle history derived from canonical NEAR actions", body = VenearLockupEventsListResponse),
        (status = 404, description = "Lockup not found", body = crate::errors::ApiError),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_lockup_events(
    State(app_state): State<AppState>,
    Path(lockup_account_id): Path<String>,
    Query(query): Query<VenearLockupEventsQuery>,
) -> Result<Json<VenearLockupEventsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let event_type_filter =
        normalized_text_filter(query.event_type.as_deref()).map(|value| value.to_ascii_lowercase());

    let mut events = load_lockup_events(db.as_ref()).await?;
    events.retain(|event| {
        if event.lockup_account_id != lockup_account_id {
            return false;
        }
        if let Some(filter) = event_type_filter.as_deref() {
            if event.event_type != filter {
                return false;
            }
        }
        true
    });

    if events.is_empty() {
        let known = load_lockup_projections(db.as_ref())
            .await?
            .into_iter()
            .any(|lockup| lockup.lockup_account_id == lockup_account_id);
        if !known {
            return Err(ApiErrorResponse::not_found("Lockup not found"));
        }
    }

    events.sort_by(|left, right| {
        right
            .refs
            .block_height
            .cmp(&left.refs.block_height)
            .then_with(|| right.refs.receipt_id.cmp(&left.refs.receipt_id))
    });

    let total = events.len();
    let data = events
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .map(|event| VenearLockupEventDto {
            lockup_account_id: event.lockup_account_id,
            owner_account_id: event.owner_account_id,
            event_type: event.event_type,
            locked_near_balance: amount_from_optional_raw(event.locked_near_balance_raw),
            refs: event.refs,
            payload: event.payload,
        })
        .collect::<Vec<_>>();

    paginate_lockup_events(data, cursor.offset, limit, total).map(Json)
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/accounts/{account_id}/lockup_events",
    params(VenearAccountLockupEventsQuery),
    tag = "venear",
    operation_id = "listVenearAccountLockupEventsCanonical",
    responses(
        (status = 200, description = "Lockup lifecycle events scoped to one account", body = VenearAccountLockupEventsListResponse),
        (status = 404, description = "Account not found", body = crate::errors::ApiError),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_account_lockup_events(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
    Query(query): Query<VenearAccountLockupEventsQuery>,
) -> Result<Json<VenearAccountLockupEventsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let event_type_filter =
        normalized_text_filter(query.event_type.as_deref()).map(|value| value.to_ascii_lowercase());

    let mut events = load_lockup_events(db.as_ref()).await?;
    events.retain(|event| {
        if event.owner_account_id != account_id {
            return false;
        }
        if let Some(filter) = event_type_filter.as_deref() {
            if event.event_type != filter {
                return false;
            }
        }
        true
    });

    if events.is_empty() {
        ensure_account_exists(db.as_ref(), &account_id).await?;
    }

    events.sort_by(|left, right| {
        right
            .refs
            .block_height
            .cmp(&left.refs.block_height)
            .then_with(|| right.refs.receipt_id.cmp(&left.refs.receipt_id))
    });

    let total = events.len();
    let data = events
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .map(|event| VenearLockupEventDto {
            lockup_account_id: event.lockup_account_id,
            owner_account_id: event.owner_account_id,
            event_type: event.event_type,
            locked_near_balance: amount_from_optional_raw(event.locked_near_balance_raw),
            refs: event.refs,
            payload: event.payload,
        })
        .collect::<Vec<_>>();

    paginate_account_lockup_events(data, cursor.offset, limit, total).map(Json)
}

pub(crate) async fn load_lockup_events(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<LockupEventProjection>, ApiErrorResponse> {
    let actions = load_canonical_actions(
        db,
        &[
            "on_lockup_deployed",
            "on_lockup_update",
            "lock_near",
            "begin_unlock_near",
            "end_unlock_near",
            "delete_lockup",
        ],
    )
    .await?;

    let mut owner_by_lockup = HashMap::<String, String>::new();
    let mut contract_by_lockup = HashMap::<String, String>::new();
    let mut events = Vec::new();

    for action in actions
        .into_iter()
        .filter(|action| is_success_status(action.status.as_deref()))
    {
        let owner_account_id = match action.method_name.as_str() {
            "on_lockup_deployed" => action
                .args_json
                .get("account_id")
                .and_then(json_value_to_string),
            "on_lockup_update" => action
                .args_json
                .get("owner_account_id")
                .and_then(json_value_to_string),
            _ if action.contract_id.contains(".venear.dao")
                && action.contract_id != "venear.dao" =>
            {
                owner_by_lockup.get(&action.contract_id).cloned()
            }
            _ => None,
        };

        let Some(owner_account_id) = owner_account_id else {
            continue;
        };
        let root_contract_id =
            if action.contract_id.contains(".venear.dao") && action.contract_id != "venear.dao" {
                contract_by_lockup
                    .get(&action.contract_id)
                    .cloned()
                    .unwrap_or_else(|| "venear.dao".to_string())
            } else {
                action.contract_id.clone()
            };
        let lockup_account_id = if action.method_name == "on_lockup_deployed" {
            derive_lockup_account_id(&owner_account_id, &root_contract_id)
        } else if action.contract_id.contains(".venear.dao")
            && action.contract_id != root_contract_id
        {
            action.contract_id.clone()
        } else {
            derive_lockup_account_id(&owner_account_id, &root_contract_id)
        };

        owner_by_lockup.insert(lockup_account_id.clone(), owner_account_id.clone());
        contract_by_lockup.insert(lockup_account_id.clone(), root_contract_id.clone());

        let locked_near_balance_raw = action
            .logs
            .iter()
            .find_map(|log| {
                log.payload
                    .get("data")
                    .and_then(|value| value.get(0))
                    .and_then(|value| value.get("locked_near_balance"))
                    .and_then(json_value_to_string)
            })
            .or_else(|| {
                action
                    .args_json
                    .get("amount")
                    .and_then(json_value_to_string)
            });

        let event_type = match action.method_name.as_str() {
            "on_lockup_deployed" => "lockup_deployed".to_string(),
            "on_lockup_update" => "lockup_update".to_string(),
            "delete_lockup" => "lockup_delete".to_string(),
            other => other.to_string(),
        };

        events.push(LockupEventProjection {
            lockup_account_id,
            owner_account_id,
            contract_id: root_contract_id,
            event_type,
            locked_near_balance_raw,
            block_timestamp: action.block_timestamp,
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
            payload: action.args_json.clone(),
        });
    }

    Ok(events)
}

pub(crate) async fn load_lockup_projections(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<LockupProjection>, ApiErrorResponse> {
    let events = load_lockup_events(db).await?;
    let active_delegations = load_active_delegations(db).await?;
    let delegated_to_by_owner = active_delegations
        .into_iter()
        .map(|edge| (edge.delegator_account_id, edge.delegatee_account_id))
        .collect::<HashMap<_, _>>();

    let mut latest_by_lockup = HashMap::<String, Vec<LockupEventProjection>>::new();
    for event in events {
        latest_by_lockup
            .entry(event.lockup_account_id.clone())
            .or_default()
            .push(event);
    }

    let mut projections = Vec::new();
    for (_, mut events) in latest_by_lockup {
        events.sort_by(|left, right| {
            left.block_timestamp
                .cmp(&right.block_timestamp)
                .then_with(|| left.refs.receipt_id.cmp(&right.refs.receipt_id))
        });
        let latest = events.last().cloned().expect("lockup event exists");
        let deployment_refs = events
            .iter()
            .find(|event| event.event_type == "lockup_deployed")
            .map(|event| event.refs.clone());
        let status = if latest.event_type == "lockup_delete" {
            "deleted"
        } else if events
            .iter()
            .rev()
            .find(|event| {
                matches!(
                    event.event_type.as_str(),
                    "begin_unlock_near" | "end_unlock_near"
                )
            })
            .map(|event| event.event_type == "begin_unlock_near")
            .unwrap_or(false)
        {
            "unlocking"
        } else {
            "active"
        }
        .to_string();
        let locked_near_balance_raw = events
            .iter()
            .rev()
            .find_map(|event| event.locked_near_balance_raw.clone())
            .unwrap_or_else(|| "0".to_string());

        projections.push(LockupProjection {
            lockup_account_id: latest.lockup_account_id.clone(),
            owner_account_id: latest.owner_account_id.clone(),
            contract_id: latest.contract_id.clone(),
            status,
            locked_near_balance_raw,
            delegated_to_account_id: delegated_to_by_owner.get(&latest.owner_account_id).cloned(),
            deployment_refs,
            latest_refs: latest.refs,
        });
    }

    Ok(projections)
}

fn paginate_lockups(
    mut data: Vec<VenearLockupSummaryDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearLockupListResponse, ApiErrorResponse> {
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
    Ok(VenearLockupListResponse {
        data,
        next_cursor,
        has_more,
    })
}

fn paginate_lockup_events(
    mut data: Vec<VenearLockupEventDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearLockupEventsListResponse, ApiErrorResponse> {
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
    Ok(VenearLockupEventsListResponse {
        data,
        next_cursor,
        has_more,
    })
}

fn paginate_account_lockup_events(
    mut data: Vec<VenearLockupEventDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearAccountLockupEventsListResponse, ApiErrorResponse> {
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
    Ok(VenearAccountLockupEventsListResponse {
        data,
        next_cursor,
        has_more,
    })
}
