use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{
    accounts::load_reward_events,
    admin::load_admin_action_projections,
    config::load_config_history,
    delegations::load_delegation_history,
    lockups::load_lockup_events,
    proposals::{load_proposal_projections, load_vote_history, proposal_timeline_events},
    sql::{
        decode_cursor, encode_cursor, normalized_contract_id_filter, normalized_event_type_filter,
        normalized_text_filter, require_db, resolve_limit,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffsetCursor {
    offset: usize,
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/activity",
    params(VenearActivityQuery),
    tag = "venear",
    operation_id = "listVenearActivityCanonical",
    responses(
        (status = 200, description = "Normalized cross-domain veNEAR activity feed derived from canonical NEAR actions", body = VenearActivityListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_activity(
    State(app_state): State<AppState>,
    Query(query): Query<VenearActivityQuery>,
) -> Result<Json<VenearActivityListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let kind_filter =
        normalized_text_filter(query.kind.as_deref()).map(|value| value.to_ascii_lowercase());
    let account_filter = normalized_text_filter(query.account_id.as_deref());
    let contract_filter = normalized_contract_id_filter(query.contract_id.as_deref());
    let event_type_filter = normalized_event_type_filter(query.event_type.as_deref());

    let mut items = Vec::<VenearActivityItemDto>::new();

    for proposal in load_proposal_projections(db.as_ref()).await? {
        for event in proposal_timeline_events(&proposal)? {
            items.push(VenearActivityItemDto {
                kind: "proposal".to_string(),
                event_type: event.event_type,
                actor_account_id: event.actor_account_id,
                subject_account_id: Some(proposal.proposer_id.clone()),
                proposal_id: Some(proposal.proposal_id),
                success: true,
                refs: event.refs,
                payload: event.payload,
            });
        }
    }

    for vote in load_vote_history(db.as_ref()).await? {
        items.push(VenearActivityItemDto {
            kind: "vote".to_string(),
            event_type: vote.event_type,
            actor_account_id: Some(vote.voter_account_id.clone()),
            subject_account_id: Some(vote.voter_account_id),
            proposal_id: Some(vote.proposal_id),
            success: true,
            refs: vote.refs,
            payload: json!({
                "vote_option": vote.vote_option,
                "voting_power_raw": vote.voting_power_raw,
                "effective_after_event": vote.effective_after_event,
            }),
        });
    }

    for event in load_delegation_history(db.as_ref()).await? {
        items.push(VenearActivityItemDto {
            kind: "delegation".to_string(),
            event_type: event.event_type,
            actor_account_id: Some(event.delegator_account_id.clone()),
            subject_account_id: event.delegatee_account_id.clone(),
            proposal_id: None,
            success: true,
            refs: event.refs,
            payload: json!({
                "delegator_account_id": event.delegator_account_id,
                "delegatee_account_id": event.delegatee_account_id,
                "delegated_voting_power_raw": event.delegated_voting_power_raw,
            }),
        });
    }

    for event in load_lockup_events(db.as_ref()).await? {
        items.push(VenearActivityItemDto {
            kind: "lockup".to_string(),
            event_type: event.event_type,
            actor_account_id: Some(event.owner_account_id.clone()),
            subject_account_id: Some(event.owner_account_id),
            proposal_id: None,
            success: true,
            refs: event.refs,
            payload: event.payload,
        });
    }

    for event in load_reward_events(db.as_ref()).await? {
        items.push(VenearActivityItemDto {
            kind: "reward".to_string(),
            event_type: event.event_type,
            actor_account_id: Some(event.account_id.clone()),
            subject_account_id: Some(event.account_id),
            proposal_id: None,
            success: true,
            refs: event.refs,
            payload: event.payload,
        });
    }

    for action in load_admin_action_projections(db.as_ref()).await? {
        items.push(VenearActivityItemDto {
            kind: "admin".to_string(),
            event_type: action.event_type,
            actor_account_id: action.actor_account_id.clone(),
            subject_account_id: action.actor_account_id,
            proposal_id: None,
            success: action.success,
            refs: action.refs,
            payload: action.payload,
        });
    }

    for entry in load_config_history(db.as_ref()).await? {
        items.push(VenearActivityItemDto {
            kind: "config".to_string(),
            event_type: entry.method_name.clone(),
            actor_account_id: Some(entry.actor_account_id.clone()),
            subject_account_id: Some(entry.actor_account_id),
            proposal_id: None,
            success: true,
            refs: entry.refs,
            payload: json!({
                "field": entry.field,
                "value": entry.value,
                "contract_id": entry.contract_id,
            }),
        });
    }

    items.retain(|item| {
        if let Some(kind) = kind_filter.as_deref() {
            if item.kind != kind {
                return false;
            }
        }
        if let Some(account_id) = account_filter.as_deref() {
            let matches_actor = item.actor_account_id.as_deref() == Some(account_id);
            let matches_subject = item.subject_account_id.as_deref() == Some(account_id);
            if !matches_actor && !matches_subject {
                return false;
            }
        }
        if let Some(proposal_id) = query.proposal_id {
            if item.proposal_id != Some(proposal_id) {
                return false;
            }
        }
        if let Some(contract_id) = contract_filter.as_deref() {
            if item.refs.contract_id != contract_id {
                return false;
            }
        }
        if let Some(event_type) = event_type_filter.as_deref() {
            if item.event_type != event_type {
                return false;
            }
        }
        if let Some(success) = query.success {
            if item.success != success {
                return false;
            }
        }
        true
    });
    items.sort_by(|left, right| {
        right
            .refs
            .block_height
            .cmp(&left.refs.block_height)
            .then_with(|| right.refs.receipt_id.cmp(&left.refs.receipt_id))
    });

    let total = items.len();
    let data = items
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .collect::<Vec<_>>();

    paginate(data, cursor.offset, limit, total).map(Json)
}

fn paginate(
    mut data: Vec<VenearActivityItemDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearActivityListResponse, ApiErrorResponse> {
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
    Ok(VenearActivityListResponse {
        data,
        next_cursor,
        has_more,
    })
}
