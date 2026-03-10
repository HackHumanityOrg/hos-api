use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::{
    accounts::ensure_account_exists,
    sql::{
        amount_from_raw, chain_ref, decode_cursor, encode_cursor, is_success_status,
        json_value_to_string, load_canonical_actions, normalized_event_type_filter,
        normalized_text_filter, parse_direction_filter, require_db, resolve_limit,
        timestamp_from_ns_opt,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffsetCursor {
    offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct DelegationHistoryProjection {
    pub delegator_account_id: String,
    pub delegatee_account_id: Option<String>,
    pub event_type: String,
    pub delegated_voting_power_raw: String,
    pub block_timestamp: i64,
    pub refs: VenearChainRefDto,
}

#[derive(Debug, Clone)]
pub(crate) struct DelegationEdgeProjection {
    pub delegator_account_id: String,
    pub delegatee_account_id: String,
    pub delegated_voting_power_raw: String,
    pub started_at: String,
    pub last_changed_at: String,
    pub refs: VenearChainRefDto,
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/delegations",
    params(VenearDelegationsQueryV2),
    tag = "venear",
    operation_id = "listVenearDelegationsCanonical",
    responses(
        (status = 200, description = "Current active delegations derived from canonical NEAR actions", body = VenearDelegationListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_delegations(
    State(app_state): State<AppState>,
    Query(query): Query<VenearDelegationsQueryV2>,
) -> Result<Json<VenearDelegationListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let delegator_filter = normalized_text_filter(query.delegator_account_id.as_deref());
    let delegatee_filter = normalized_text_filter(query.delegatee_account_id.as_deref());

    let mut edges = load_active_delegations(db.as_ref()).await?;
    edges.retain(|edge| {
        if let Some(delegator) = delegator_filter.as_deref() {
            if edge.delegator_account_id != delegator {
                return false;
            }
        }
        if let Some(delegatee) = delegatee_filter.as_deref() {
            if edge.delegatee_account_id != delegatee {
                return false;
            }
        }
        true
    });
    edges.sort_by(|left, right| {
        left.delegator_account_id
            .cmp(&right.delegator_account_id)
            .then_with(|| left.delegatee_account_id.cmp(&right.delegatee_account_id))
    });

    let total = edges.len();
    let data = edges
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .map(|edge| VenearDelegationEdgeDto {
            delegator_account_id: edge.delegator_account_id,
            delegatee_account_id: edge.delegatee_account_id,
            last_observed_delegated_voting_power: amount_from_raw(edge.delegated_voting_power_raw),
            started_at: edge.started_at,
            last_changed_at: edge.last_changed_at,
            refs: edge.refs,
        })
        .collect::<Vec<_>>();

    paginate_edges(data, cursor.offset, limit, total).map(Json)
}

pub(crate) async fn load_delegation_history(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<DelegationHistoryProjection>, ApiErrorResponse> {
    let actions = load_canonical_actions(db, &["delegate_all", "undelegate"]).await?;
    let mut history = actions
        .into_iter()
        .filter(|action| {
            matches!(action.method_name.as_str(), "delegate_all" | "undelegate")
                && is_success_status(action.status.as_deref())
        })
        .map(|action| {
            let delegatee_from_args = action
                .args_json
                .get("receiver_id")
                .and_then(json_value_to_string);
            let delegated_voting_power_raw = action
                .logs
                .iter()
                .find_map(
                    |log| match (action.method_name.as_str(), log.event_type.as_str()) {
                        ("delegate_all", "ft_mint") => {
                            let owner_id = log
                                .payload
                                .get("data")
                                .and_then(|value| value.get(0))
                                .and_then(|value| value.get("owner_id"))
                                .and_then(json_value_to_string);
                            if owner_id == delegatee_from_args {
                                log.payload
                                    .get("data")
                                    .and_then(|value| value.get(0))
                                    .and_then(|value| value.get("amount"))
                                    .and_then(json_value_to_string)
                            } else {
                                None
                            }
                        }
                        ("delegate_all", "ft_burn") => log
                            .payload
                            .get("data")
                            .and_then(|value| value.get(0))
                            .and_then(|value| value.get("amount"))
                            .and_then(json_value_to_string),
                        ("undelegate", "ft_mint") => {
                            let owner_id = log
                                .payload
                                .get("data")
                                .and_then(|value| value.get(0))
                                .and_then(|value| value.get("owner_id"))
                                .and_then(json_value_to_string);
                            if owner_id.as_deref() == Some(action.predecessor_id.as_str()) {
                                log.payload
                                    .get("data")
                                    .and_then(|value| value.get(0))
                                    .and_then(|value| value.get("amount"))
                                    .and_then(json_value_to_string)
                            } else {
                                None
                            }
                        }
                        ("undelegate", "ft_burn") => log
                            .payload
                            .get("data")
                            .and_then(|value| value.get(0))
                            .and_then(|value| value.get("amount"))
                            .and_then(json_value_to_string),
                        _ => None,
                    },
                )
                .unwrap_or_else(|| "0".to_string());

            DelegationHistoryProjection {
                delegator_account_id: action.predecessor_id.clone(),
                delegatee_account_id: delegatee_from_args,
                event_type: action.method_name.clone(),
                delegated_voting_power_raw,
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
            }
        })
        .collect::<Vec<_>>();

    history.sort_by(|left, right| {
        left.delegator_account_id
            .cmp(&right.delegator_account_id)
            .then_with(|| left.block_timestamp.cmp(&right.block_timestamp))
            .then_with(|| left.refs.receipt_id.cmp(&right.refs.receipt_id))
    });

    let mut active_delegatee = HashMap::<String, String>::new();
    for event in &mut history {
        if event.event_type == "undelegate" && event.delegatee_account_id.is_none() {
            event.delegatee_account_id = active_delegatee.get(&event.delegator_account_id).cloned();
        }

        if event.event_type == "delegate_all" {
            if let Some(delegatee) = event.delegatee_account_id.clone() {
                active_delegatee.insert(event.delegator_account_id.clone(), delegatee);
            }
        } else if event.event_type == "undelegate" {
            active_delegatee.remove(&event.delegator_account_id);
        }
    }

    Ok(history)
}

pub(crate) async fn load_active_delegations(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<DelegationEdgeProjection>, ApiErrorResponse> {
    let history = load_delegation_history(db).await?;
    let mut latest_by_delegator = HashMap::<String, DelegationHistoryProjection>::new();
    for event in history {
        latest_by_delegator.insert(event.delegator_account_id.clone(), event);
    }

    Ok(latest_by_delegator
        .into_values()
        .filter(|event| event.event_type == "delegate_all")
        .filter_map(|event| {
            Some(DelegationEdgeProjection {
                delegator_account_id: event.delegator_account_id,
                delegatee_account_id: event.delegatee_account_id?,
                delegated_voting_power_raw: event.delegated_voting_power_raw,
                started_at: event.refs.block_timestamp.clone(),
                last_changed_at: event.refs.block_timestamp.clone(),
                refs: event.refs,
            })
        })
        .collect())
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/accounts/{account_id}/delegation_history",
    params(VenearAccountDelegationHistoryQuery),
    tag = "venear",
    operation_id = "listVenearAccountDelegationHistoryCanonical",
    responses(
        (status = 200, description = "Delegation history involving one account", body = VenearAccountDelegationHistoryListResponse),
        (status = 404, description = "Account not found", body = crate::errors::ApiError),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_account_delegation_history(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
    Query(query): Query<VenearAccountDelegationHistoryQuery>,
) -> Result<Json<VenearAccountDelegationHistoryListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let direction_filter = parse_direction_filter(query.direction.as_deref())?;
    let event_type_filter = normalized_event_type_filter(query.event_type.as_deref());
    let counterparty_filter = normalized_text_filter(query.counterparty_account_id.as_deref());

    let mut history = load_delegation_history(db.as_ref()).await?;
    history.retain(|event| {
        let direction = if event.delegator_account_id == account_id {
            "outbound"
        } else if event.delegatee_account_id.as_deref() == Some(account_id.as_str()) {
            "inbound"
        } else {
            return false;
        };
        if let Some(filter) = direction_filter.as_deref() {
            if direction != filter {
                return false;
            }
        }
        if let Some(filter) = event_type_filter.as_deref() {
            if event.event_type != filter {
                return false;
            }
        }
        if let Some(counterparty) = counterparty_filter.as_deref() {
            let resolved = if direction == "outbound" {
                event.delegatee_account_id.as_deref()
            } else {
                Some(event.delegator_account_id.as_str())
            };
            if resolved != Some(counterparty) {
                return false;
            }
        }
        true
    });

    if history.is_empty() {
        ensure_account_exists(db.as_ref(), &account_id).await?;
    }

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
        .map(|event| {
            let direction = if event.delegator_account_id == account_id {
                "outbound"
            } else {
                "inbound"
            };
            VenearAccountDelegationHistoryEventDto {
                account_id: account_id.clone(),
                direction: direction.to_string(),
                event_type: event.event_type,
                counterparty_account_id: if direction == "outbound" {
                    event.delegatee_account_id
                } else {
                    Some(event.delegator_account_id)
                },
                delegated_voting_power: amount_from_raw(event.delegated_voting_power_raw),
                refs: event.refs,
            }
        })
        .collect::<Vec<_>>();

    paginate_history(data, cursor.offset, limit, total).map(Json)
}

fn paginate_edges(
    mut data: Vec<VenearDelegationEdgeDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearDelegationListResponse, ApiErrorResponse> {
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
    Ok(VenearDelegationListResponse {
        data,
        next_cursor,
        has_more,
    })
}

fn paginate_history(
    mut data: Vec<VenearAccountDelegationHistoryEventDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearAccountDelegationHistoryListResponse, ApiErrorResponse> {
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
    Ok(VenearAccountDelegationHistoryListResponse {
        data,
        next_cursor,
        has_more,
    })
}
