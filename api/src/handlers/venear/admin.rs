use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

use super::sql::{
    chain_ref, decode_cursor, encode_cursor, load_canonical_actions, normalized_contract_id_filter,
    normalized_event_type_filter, normalized_text_filter, require_db, resolve_limit,
    timestamp_from_ns_opt,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffsetCursor {
    offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct AdminActionProjection {
    pub contract_id: String,
    pub event_type: String,
    pub actor_account_id: Option<String>,
    pub success: bool,
    pub refs: VenearChainRefDto,
    pub payload: serde_json::Value,
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/admin/actions",
    summary = "List veNEAR admin actions",
    description = "Returns admin and operator actions derived from canonical NEAR actions, with cursor pagination and optional contract, event-type, actor, and success filters.",
    params(VenearAdminActionsQuery),
    tag = "venear",
    operation_id = "listVenearAdminActionsCanonical",
    responses(
        (status = 200, description = "Admin and operator actions derived from canonical NEAR actions", body = VenearAdminActionsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_admin_actions(
    State(app_state): State<AppState>,
    Query(query): Query<VenearAdminActionsQuery>,
) -> Result<Json<VenearAdminActionsListResponse>, ApiErrorResponse> {
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
    let event_filter = normalized_event_type_filter(query.event_type.as_deref());
    let actor_filter = normalized_text_filter(query.actor_account_id.as_deref());

    let mut actions = load_admin_action_projections(db.as_ref()).await?;
    actions.retain(|action| {
        if let Some(contract_id) = contract_filter.as_deref() {
            if action.contract_id != contract_id {
                return false;
            }
        }
        if let Some(event_type) = event_filter.as_deref() {
            if action.event_type != event_type {
                return false;
            }
        }
        if let Some(actor_account_id) = actor_filter.as_deref() {
            if action.actor_account_id.as_deref() != Some(actor_account_id) {
                return false;
            }
        }
        if let Some(success) = query.success {
            if action.success != success {
                return false;
            }
        }
        true
    });
    actions.sort_by(|left, right| {
        right
            .refs
            .block_height
            .cmp(&left.refs.block_height)
            .then_with(|| right.refs.receipt_id.cmp(&left.refs.receipt_id))
    });

    let total = actions.len();
    let data = actions
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .map(|action| VenearAdminActionDto {
            contract_id: action.contract_id,
            event_type: action.event_type,
            actor_account_id: action.actor_account_id,
            success: action.success,
            refs: action.refs,
            payload: action.payload,
        })
        .collect::<Vec<_>>();

    paginate(data, cursor.offset, limit, total).map(Json)
}

pub(crate) async fn load_admin_action_projections(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<AdminActionProjection>, ApiErrorResponse> {
    let admin_methods = [
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
        "upgrade",
        "prepare_lockup_code",
        "migrate_state",
    ];

    let allowed = admin_methods.iter().copied().collect::<HashSet<_>>();
    let actions = load_canonical_actions(db, &admin_methods).await?;
    Ok(actions
        .into_iter()
        .filter(|action| allowed.contains(action.method_name.as_str()))
        .map(|action| AdminActionProjection {
            contract_id: action.contract_id.clone(),
            event_type: action.method_name.clone(),
            actor_account_id: Some(action.predecessor_id.clone()),
            success: matches!(
                action.status.as_deref(),
                Some("SuccessValue" | "SuccessReceiptId")
            ),
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
            payload: action.args_json,
        })
        .collect())
}

fn paginate(
    mut data: Vec<VenearAdminActionDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearAdminActionsListResponse, ApiErrorResponse> {
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
    Ok(VenearAdminActionsListResponse {
        data,
        next_cursor,
        has_more,
    })
}
