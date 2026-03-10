use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};

use super::sql::{
    amount_from_raw, chain_ref, compute_proposal_status, decode_cursor, encode_cursor,
    is_success_status, json_value_to_i64, json_value_to_string, load_canonical_actions,
    load_receipt_edges, normalized_contract_id_filter, normalized_event_type_filter,
    normalized_text_filter, parse_rfc3339_to_ns, require_db, resolve_limit, timestamp_from_ns_opt,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffsetCursor {
    offset: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct VoteHistoryProjection {
    pub proposal_id: i64,
    pub voter_account_id: String,
    pub event_type: String,
    pub vote_option: Option<i32>,
    pub voting_power_raw: String,
    pub log_index: i64,
    pub block_timestamp: i64,
    pub refs: VenearChainRefDto,
    pub effective_after_event: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct ProposalProjection {
    pub proposal_id: i64,
    pub contract_id: String,
    pub proposer_id: String,
    pub reviewer_id: Option<String>,
    pub metadata: JsonValue,
    pub voting_options: Vec<String>,
    pub status: String,
    pub created_at_ns: i64,
    pub approved_at_ns: Option<i64>,
    pub rejected_at_ns: Option<i64>,
    pub voting_start_at_ns: Option<i64>,
    pub voting_duration_ns: Option<i64>,
    pub snapshot_present: bool,
    pub snapshot_block_height: Option<i64>,
    pub snapshot_root: Option<String>,
    pub snapshot_total_venear_raw: String,
    pub tally: Vec<VenearProposalTallyDto>,
    pub turnout_voter_count: i64,
    pub turnout_voting_power_raw: String,
    pub latest_event_type: String,
    pub latest_refs: VenearChainRefDto,
    pub create_refs: VenearChainRefDto,
    pub approve_requested_refs: Option<VenearChainRefDto>,
    pub approve_finalized_refs: Option<VenearChainRefDto>,
    pub reject_refs: Option<VenearChainRefDto>,
    pub approve_requested_payload: Option<JsonValue>,
    pub approve_finalized_payload: Option<JsonValue>,
    pub reject_payload: Option<JsonValue>,
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/proposals",
    params(VenearProposalListQuery),
    tag = "venear",
    operation_id = "listVenearProposalsCanonical",
    responses(
        (status = 200, description = "Curated proposal summaries derived from canonical NEAR actions", body = VenearProposalListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_proposals(
    State(app_state): State<AppState>,
    Query(query): Query<VenearProposalListQuery>,
) -> Result<Json<VenearProposalListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });

    let status_filter =
        normalized_text_filter(query.status.as_deref()).map(|v| v.to_ascii_lowercase());
    let proposer_filter = normalized_text_filter(query.proposer_id.as_deref());
    let reviewer_filter = normalized_text_filter(query.reviewer_id.as_deref());
    let contract_filter = normalized_contract_id_filter(query.contract_id.as_deref());
    let q_filter = normalized_text_filter(query.q.as_deref()).map(|v| v.to_ascii_lowercase());
    let created_after_ns = parse_rfc3339_to_ns(query.created_after.as_deref())?;
    let created_before_ns = parse_rfc3339_to_ns(query.created_before.as_deref())?;

    let mut proposals = load_proposal_projections(db.as_ref()).await?;
    proposals.retain(|proposal| {
        if let Some(status) = status_filter.as_deref() {
            if proposal.status != status {
                return false;
            }
        }
        if let Some(proposer_id) = proposer_filter.as_deref() {
            if proposal.proposer_id != proposer_id {
                return false;
            }
        }
        if let Some(reviewer_id) = reviewer_filter.as_deref() {
            if proposal.reviewer_id.as_deref() != Some(reviewer_id) {
                return false;
            }
        }
        if let Some(contract_id) = contract_filter.as_deref() {
            if proposal.contract_id != contract_id {
                return false;
            }
        }
        if let Some(has_votes) = query.has_votes {
            if (proposal.turnout_voter_count > 0) != has_votes {
                return false;
            }
        }
        if let Some(created_after_ns) = created_after_ns {
            if proposal.created_at_ns < created_after_ns {
                return false;
            }
        }
        if let Some(created_before_ns) = created_before_ns {
            if proposal.created_at_ns > created_before_ns {
                return false;
            }
        }
        if let Some(query_text) = q_filter.as_deref() {
            let haystack = [
                proposal_title(&proposal.metadata),
                proposal_description(&proposal.metadata),
                proposal_link(&proposal.metadata),
            ]
            .into_iter()
            .flatten()
            .map(|value| value.to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join("\n");
            if !haystack.contains(query_text) {
                return false;
            }
        }
        true
    });

    proposals.sort_by(|left, right| {
        right
            .latest_refs
            .block_height
            .cmp(&left.latest_refs.block_height)
            .then_with(|| right.proposal_id.cmp(&left.proposal_id))
    });

    let total = proposals.len();
    let data = proposals
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .map(projection_to_summary_dto)
        .collect::<Result<Vec<_>, _>>()?;

    paginate_with_offset(data, cursor.offset, limit, total).map(Json)
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/proposals/{proposal_id}",
    tag = "venear",
    operation_id = "getVenearProposalCanonical",
    responses(
        (status = 200, description = "Full proposal detail derived from canonical NEAR actions", body = VenearProposalDetailDto),
        (status = 404, description = "Proposal not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_proposal(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<i64>,
) -> Result<Json<VenearProposalDetailDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let proposal = load_proposal_projections(db.as_ref())
        .await?
        .into_iter()
        .find(|proposal| proposal.proposal_id == proposal_id)
        .ok_or_else(|| ApiErrorResponse::not_found("Proposal not found"))?;

    Ok(Json(projection_to_detail_dto(&proposal)?))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/proposals/{proposal_id}/timeline",
    params(VenearProposalTimelineQuery),
    tag = "venear",
    operation_id = "listVenearProposalTimelineCanonical",
    responses(
        (status = 200, description = "Proposal lifecycle events derived from canonical NEAR actions", body = VenearProposalTimelineListResponse),
        (status = 404, description = "Proposal not found", body = crate::errors::ApiError),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_proposal_timeline(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<i64>,
    Query(query): Query<VenearProposalTimelineQuery>,
) -> Result<Json<VenearProposalTimelineListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let event_type_filter = normalized_event_type_filter(query.event_type.as_deref());
    let actor_filter = normalized_text_filter(query.actor_account_id.as_deref());

    let proposal = load_proposal_projections(db.as_ref())
        .await?
        .into_iter()
        .find(|proposal| proposal.proposal_id == proposal_id)
        .ok_or_else(|| ApiErrorResponse::not_found("Proposal not found"))?;

    let mut events = proposal_timeline_events(&proposal)?;
    events.retain(|event| {
        if let Some(event_type) = event_type_filter.as_deref() {
            if event.event_type != event_type {
                return false;
            }
        }
        if let Some(actor) = actor_filter.as_deref() {
            if event.actor_account_id.as_deref() != Some(actor) {
                return false;
            }
        }
        true
    });
    events.sort_by(|left, right| {
        right
            .refs
            .block_height
            .cmp(&left.refs.block_height)
            .then_with(|| right.proposal_id.cmp(&left.proposal_id))
    });

    let total = events.len();
    let data = events
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .collect::<Vec<_>>();

    paginate_with_offset_timeline(data, cursor.offset, limit, total).map(Json)
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/proposals/{proposal_id}/votes",
    params(VenearProposalVotesQueryV2),
    tag = "venear",
    operation_id = "listVenearProposalVotesCanonical",
    responses(
        (status = 200, description = "Current effective proposal votes derived from canonical NEAR actions", body = VenearProposalVoteListResponse),
        (status = 404, description = "Proposal not found", body = crate::errors::ApiError),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_proposal_votes(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<i64>,
    Query(query): Query<VenearProposalVotesQueryV2>,
) -> Result<Json<VenearProposalVoteListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let voter_filter = normalized_text_filter(query.voter_account_id.as_deref());

    let proposal = load_proposal_projections(db.as_ref())
        .await?
        .into_iter()
        .find(|proposal| proposal.proposal_id == proposal_id)
        .ok_or_else(|| ApiErrorResponse::not_found("Proposal not found"))?;

    let mut latest = effective_vote_events(load_vote_history(db.as_ref()).await?);
    latest.retain(|event| {
        if event.proposal_id != proposal_id {
            return false;
        }
        if let Some(voter) = voter_filter.as_deref() {
            if event.voter_account_id != voter {
                return false;
            }
        }
        if let Some(vote_option) = query.vote_option {
            if event.vote_option != Some(vote_option) {
                return false;
            }
        }
        true
    });
    latest.sort_by(|left, right| {
        right
            .refs
            .block_height
            .cmp(&left.refs.block_height)
            .then_with(|| left.voter_account_id.cmp(&right.voter_account_id))
    });

    let total = latest.len();
    let data = latest
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .map(|event| vote_projection_to_vote_dto(&event, &proposal.voting_options))
        .collect::<Vec<_>>();

    paginate_with_offset_votes(data, cursor.offset, limit, total).map(Json)
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/proposals/{proposal_id}/vote_history",
    params(VenearProposalVoteHistoryQuery),
    tag = "venear",
    operation_id = "listVenearProposalVoteHistoryCanonical",
    responses(
        (status = 200, description = "Full proposal vote history derived from canonical NEAR actions", body = VenearProposalVoteHistoryListResponse),
        (status = 404, description = "Proposal not found", body = crate::errors::ApiError),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_proposal_vote_history(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<i64>,
    Query(query): Query<VenearProposalVoteHistoryQuery>,
) -> Result<Json<VenearProposalVoteHistoryListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let voter_filter = normalized_text_filter(query.voter_account_id.as_deref());
    let event_type_filter = normalized_event_type_filter(query.event_type.as_deref());

    let proposal = load_proposal_projections(db.as_ref())
        .await?
        .into_iter()
        .find(|proposal| proposal.proposal_id == proposal_id)
        .ok_or_else(|| ApiErrorResponse::not_found("Proposal not found"))?;

    let mut history = load_vote_history(db.as_ref()).await?;
    history.retain(|event| {
        if event.proposal_id != proposal_id {
            return false;
        }
        if let Some(voter) = voter_filter.as_deref() {
            if event.voter_account_id != voter {
                return false;
            }
        }
        if let Some(vote_option) = query.vote_option {
            if event.vote_option != Some(vote_option) {
                return false;
            }
        }
        if let Some(event_type) = event_type_filter.as_deref() {
            if event.event_type != event_type {
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
            .then_with(|| right.log_index.cmp(&left.log_index))
    });

    let total = history.len();
    let data = history
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .map(|event| vote_projection_to_history_dto(&event, &proposal.voting_options))
        .collect::<Vec<_>>();

    paginate_with_offset_vote_history(data, cursor.offset, limit, total).map(Json)
}

pub(crate) async fn load_vote_history(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<VoteHistoryProjection>, ApiErrorResponse> {
    let actions = load_canonical_actions(db, &["vote"]).await?;
    let mut history = actions
        .into_iter()
        .filter(|action| {
            action.method_name == "vote" && is_success_status(action.status.as_deref())
        })
        .flat_map(|action| {
            let proposal_id = action
                .args_json
                .get("proposal_id")
                .and_then(json_value_to_i64)
                .unwrap_or_default();
            let fallback_vote_option = action
                .args_json
                .get("vote")
                .and_then(json_value_to_i64)
                .and_then(|value| i32::try_from(value).ok());

            action
                .logs
                .iter()
                .filter(|log| matches!(log.event_type.as_str(), "add_vote" | "remove_vote"))
                .map(|log| {
                    let log_vote_option = log
                        .payload
                        .get("data")
                        .and_then(|value| value.get(0))
                        .and_then(|value| value.get("vote"))
                        .and_then(json_value_to_i64)
                        .and_then(|value| i32::try_from(value).ok());

                    VoteHistoryProjection {
                        proposal_id,
                        voter_account_id: action.predecessor_id.clone(),
                        event_type: log.event_type.clone(),
                        vote_option: log_vote_option.or(fallback_vote_option),
                        voting_power_raw: log
                            .payload
                            .get("data")
                            .and_then(|value| value.get(0))
                            .and_then(|value| value.get("account_balance"))
                            .and_then(json_value_to_string)
                            .unwrap_or_else(|| "0".to_string()),
                        log_index: log.log_index,
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
                        effective_after_event: false,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    history.sort_by(|left, right| {
        left.proposal_id
            .cmp(&right.proposal_id)
            .then_with(|| left.voter_account_id.cmp(&right.voter_account_id))
            .then_with(|| left.block_timestamp.cmp(&right.block_timestamp))
            .then_with(|| left.refs.receipt_id.cmp(&right.refs.receipt_id))
            .then_with(|| left.log_index.cmp(&right.log_index))
    });

    let latest_keys = history
        .iter()
        .rev()
        .map(|event| {
            (
                (event.proposal_id, event.voter_account_id.clone()),
                event.event_type.clone(),
            )
        })
        .collect::<HashMap<_, _>>();

    for event in &mut history {
        event.effective_after_event = latest_keys
            .get(&(event.proposal_id, event.voter_account_id.clone()))
            .map(|event_type| event_type == "add_vote" && *event_type == event.event_type)
            .unwrap_or(false);
    }

    Ok(history)
}

pub(crate) fn effective_vote_events(
    history: Vec<VoteHistoryProjection>,
) -> Vec<VoteHistoryProjection> {
    let mut latest = HashSet::new();
    let mut effective = Vec::new();
    for event in history.into_iter().rev() {
        let key = (event.proposal_id, event.voter_account_id.clone());
        if latest.insert(key) && event.event_type == "add_vote" {
            effective.push(event);
        }
    }
    effective
}

pub(crate) async fn load_proposal_projections(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<ProposalProjection>, ApiErrorResponse> {
    let actions = load_canonical_actions(
        db,
        &[
            "create_proposal",
            "approve_proposal",
            "reject_proposal",
            "on_get_snapshot",
            "vote",
        ],
    )
    .await?;
    let vote_history = load_vote_history(db).await?;

    let create_actions = actions
        .iter()
        .filter(|action| {
            action.method_name == "create_proposal" && is_success_status(action.status.as_deref())
        })
        .cloned()
        .collect::<Vec<_>>();

    let approve_actions = actions
        .iter()
        .filter(|action| {
            action.method_name == "approve_proposal" && is_success_status(action.status.as_deref())
        })
        .cloned()
        .collect::<Vec<_>>();

    let reject_actions = actions
        .iter()
        .filter(|action| {
            action.method_name == "reject_proposal" && is_success_status(action.status.as_deref())
        })
        .cloned()
        .collect::<Vec<_>>();

    let snapshot_actions = actions
        .iter()
        .filter(|action| {
            action.method_name == "on_get_snapshot" && is_success_status(action.status.as_deref())
        })
        .cloned()
        .collect::<Vec<_>>();

    let edge_receipt_ids = approve_actions
        .iter()
        .map(|action| action.receipt_id.clone())
        .chain(
            snapshot_actions
                .iter()
                .map(|action| action.receipt_id.clone()),
        )
        .collect::<Vec<_>>();
    let edges = load_receipt_edges(db, &edge_receipt_ids).await?;
    let parent_by_child = edges
        .into_iter()
        .map(|edge| (edge.child_receipt_id, edge.parent_receipt_id))
        .collect::<HashMap<_, _>>();

    let mut projections = Vec::new();
    for create in create_actions {
        let Some(proposal_id) = extract_proposal_id_from_create(&create) else {
            continue;
        };

        let approve_requested = approve_actions
            .iter()
            .filter(|action| {
                action
                    .args_json
                    .get("proposal_id")
                    .and_then(json_value_to_i64)
                    == Some(proposal_id)
            })
            .max_by_key(|action| (action.block_timestamp, action.receipt_id.clone()))
            .cloned();

        let approve_finalized = approve_requested.as_ref().and_then(|approve| {
            snapshot_actions
                .iter()
                .filter(|snapshot| {
                    let result_receipt_match = approve
                        .results_json
                        .as_ref()
                        .and_then(|value| value.get("receipt_id"))
                        .and_then(json_value_to_string)
                        .as_deref()
                        == Some(snapshot.receipt_id.as_str());
                    let edge_match = parent_by_child
                        .get(&snapshot.receipt_id)
                        .map(|parent| parent == &approve.receipt_id)
                        .unwrap_or(false);
                    result_receipt_match || edge_match
                })
                .max_by_key(|action| (action.block_timestamp, action.receipt_id.clone()))
                .cloned()
        });

        let reject = reject_actions
            .iter()
            .filter(|action| {
                action
                    .args_json
                    .get("proposal_id")
                    .and_then(json_value_to_i64)
                    == Some(proposal_id)
            })
            .max_by_key(|action| (action.block_timestamp, action.receipt_id.clone()))
            .cloned();

        let effective_votes = effective_vote_events(vote_history.clone())
            .into_iter()
            .filter(|event| event.proposal_id == proposal_id)
            .collect::<Vec<_>>();
        let tally = proposal_tally(
            &effective_votes,
            voting_options_from_metadata(&create.args_json),
        );
        let turnout_voter_count = effective_votes.len() as i64;
        let turnout_voting_power_raw = effective_votes
            .iter()
            .fold(0_i128, |acc, event| {
                acc + event.voting_power_raw.parse::<i128>().unwrap_or(0)
            })
            .to_string();

        let approve_requested_refs = approve_requested
            .as_ref()
            .map(action_to_chain_ref)
            .transpose()?;
        let approve_finalized_refs = approve_finalized
            .as_ref()
            .map(action_to_chain_ref)
            .transpose()?;
        let reject_refs = reject.as_ref().map(action_to_chain_ref).transpose()?;
        let create_refs = action_to_chain_ref(&create)?;

        let snapshot_results = approve_finalized
            .as_ref()
            .and_then(|action| action.results_json.clone())
            .unwrap_or(JsonValue::Null);
        let snapshot_present = snapshot_results
            .get("snapshot_and_state")
            .and_then(|value| value.as_object())
            .map(|value| !value.is_empty())
            .unwrap_or(false);
        let voting_options = voting_options_from_metadata(&create.args_json);
        let voting_duration_ns = snapshot_results
            .get("voting_duration_ns")
            .and_then(json_value_to_i64);
        let voting_start_at_ns = snapshot_results
            .get("voting_start_time_ns")
            .and_then(json_value_to_i64)
            .or_else(|| {
                approve_requested
                    .as_ref()
                    .and_then(|action| action.args_json.get("voting_start_time_sec"))
                    .and_then(json_value_to_i64)
                    .map(|seconds| seconds.saturating_mul(1_000_000_000))
            });
        let created_at_ns = snapshot_results
            .get("creation_time_ns")
            .and_then(json_value_to_i64)
            .unwrap_or(create.block_timestamp);
        let snapshot_block_height = snapshot_results
            .get("snapshot_and_state")
            .and_then(|value| value.get("snapshot"))
            .and_then(|value| value.get("block_height"))
            .and_then(json_value_to_i64);
        let snapshot_root = snapshot_results
            .get("snapshot_and_state")
            .and_then(|value| value.get("snapshot"))
            .and_then(|value| value.get("root"))
            .and_then(json_value_to_string);
        let snapshot_total_venear_raw = snapshot_results
            .get("snapshot_and_state")
            .and_then(|value| value.get("total_venear"))
            .and_then(json_value_to_string)
            .unwrap_or_else(|| "0".to_string());
        let latest_event_type = if reject.is_some() {
            "proposal_reject".to_string()
        } else if approve_finalized.is_some() {
            "proposal_approve_finalized".to_string()
        } else if approve_requested.is_some() {
            "proposal_approve_requested".to_string()
        } else {
            "create_proposal".to_string()
        };
        let latest_refs = reject_refs
            .clone()
            .or_else(|| approve_finalized_refs.clone())
            .or_else(|| approve_requested_refs.clone())
            .unwrap_or_else(|| create_refs.clone());
        let status = compute_proposal_status(
            reject.as_ref().map(|action| action.block_timestamp),
            voting_start_at_ns,
            voting_duration_ns,
            approve_finalized.is_some(),
        );

        projections.push(ProposalProjection {
            proposal_id,
            contract_id: create.contract_id.clone(),
            proposer_id: create.predecessor_id.clone(),
            reviewer_id: reject
                .as_ref()
                .map(|action| action.predecessor_id.clone())
                .or_else(|| {
                    approve_requested
                        .as_ref()
                        .map(|action| action.predecessor_id.clone())
                }),
            metadata: create
                .args_json
                .get("metadata")
                .cloned()
                .unwrap_or(JsonValue::Null),
            voting_options,
            status,
            created_at_ns,
            approved_at_ns: approve_finalized
                .as_ref()
                .map(|action| action.block_timestamp)
                .or_else(|| {
                    approve_requested
                        .as_ref()
                        .map(|action| action.block_timestamp)
                }),
            rejected_at_ns: reject.as_ref().map(|action| action.block_timestamp),
            voting_start_at_ns,
            voting_duration_ns,
            snapshot_present,
            snapshot_block_height,
            snapshot_root,
            snapshot_total_venear_raw,
            tally,
            turnout_voter_count,
            turnout_voting_power_raw,
            latest_event_type,
            latest_refs,
            create_refs,
            approve_requested_refs,
            approve_finalized_refs,
            reject_refs,
            approve_requested_payload: approve_requested
                .as_ref()
                .map(|action| action.args_json.clone()),
            approve_finalized_payload: approve_finalized
                .as_ref()
                .and_then(|action| action.results_json.clone()),
            reject_payload: reject.as_ref().map(|action| action.args_json.clone()),
        });
    }

    Ok(projections)
}

fn extract_proposal_id_from_create(action: &super::sql::CanonicalAction) -> Option<i64> {
    action.logs.iter().find_map(|log| {
        if log.event_type != "create_proposal" {
            return None;
        }
        log.payload
            .get("data")
            .and_then(|value| value.get(0))
            .and_then(|value| value.get("proposal_id"))
            .and_then(json_value_to_i64)
    })
}

fn voting_options_from_metadata(args_json: &JsonValue) -> Vec<String> {
    args_json
        .get("metadata")
        .and_then(|value| value.get("voting_options"))
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(json_value_to_string)
                .collect::<Vec<_>>()
        })
        .filter(|values| !values.is_empty())
        .unwrap_or_else(|| {
            vec![
                "For".to_string(),
                "Against".to_string(),
                "Abstain".to_string(),
            ]
        })
}

fn proposal_tally(
    votes: &[VoteHistoryProjection],
    options: Vec<String>,
) -> Vec<VenearProposalTallyDto> {
    let mut counts = HashMap::<i32, (i64, i128)>::new();
    for vote in votes {
        if let Some(option_index) = vote.vote_option {
            let entry = counts.entry(option_index).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += vote.voting_power_raw.parse::<i128>().unwrap_or(0);
        }
    }

    let mut tally = options
        .into_iter()
        .enumerate()
        .map(|(option_index, option_label)| {
            let option_index = option_index as i32;
            let (votes_count, voting_power_raw) = counts.remove(&option_index).unwrap_or((0, 0));
            VenearProposalTallyDto {
                option_index,
                option_label: Some(option_label),
                votes_count,
                voting_power: amount_from_raw(voting_power_raw.to_string()),
            }
        })
        .collect::<Vec<_>>();
    tally.extend(
        counts
            .into_iter()
            .map(
                |(option_index, (votes_count, voting_power_raw))| VenearProposalTallyDto {
                    option_index,
                    option_label: None,
                    votes_count,
                    voting_power: amount_from_raw(voting_power_raw.to_string()),
                },
            ),
    );
    tally.sort_by_key(|entry| entry.option_index);
    tally
}

fn action_to_chain_ref(
    action: &super::sql::CanonicalAction,
) -> Result<VenearChainRefDto, ApiErrorResponse> {
    Ok(chain_ref(
        action.contract_id.clone(),
        action.transaction_hash.clone(),
        action.receipt_id.clone(),
        action.execution_outcome_id.clone(),
        action.block_height,
        timestamp_from_ns_opt(Some(action.block_timestamp))?
            .unwrap_or_else(|| action.block_timestamp.to_string()),
    ))
}

fn proposal_title(metadata: &JsonValue) -> Option<String> {
    metadata.get("title").and_then(json_value_to_string)
}

fn proposal_description(metadata: &JsonValue) -> Option<String> {
    metadata.get("description").and_then(json_value_to_string)
}

fn proposal_link(metadata: &JsonValue) -> Option<String> {
    metadata.get("link").and_then(json_value_to_string)
}

pub(crate) fn projection_to_summary_dto(
    proposal: ProposalProjection,
) -> Result<VenearProposalSummaryV2Dto, ApiErrorResponse> {
    let snapshot = proposal_snapshot_dto(&proposal);

    Ok(VenearProposalSummaryV2Dto {
        proposal_id: proposal.proposal_id,
        contract_id: proposal.contract_id,
        title: proposal_title(&proposal.metadata),
        description: proposal_description(&proposal.metadata),
        link: proposal_link(&proposal.metadata),
        voting_options: proposal.voting_options,
        status: proposal.status,
        proposer_id: proposal.proposer_id,
        reviewer_id: proposal.reviewer_id,
        created_at: timestamp_from_ns_opt(Some(proposal.created_at_ns))?.unwrap_or_default(),
        approved_at: timestamp_from_ns_opt(proposal.approved_at_ns)?,
        rejected_at: timestamp_from_ns_opt(proposal.rejected_at_ns)?,
        voting_start_at: timestamp_from_ns_opt(proposal.voting_start_at_ns)?,
        voting_end_at: timestamp_from_ns_opt(
            proposal
                .voting_start_at_ns
                .zip(proposal.voting_duration_ns)
                .map(|(start, duration)| start.saturating_add(duration)),
        )?,
        snapshot,
        tally: proposal.tally,
        turnout_voter_count: proposal.turnout_voter_count,
        turnout_voting_power: amount_from_raw(proposal.turnout_voting_power_raw),
        latest_event_type: proposal.latest_event_type,
        refs: proposal.latest_refs,
    })
}

fn projection_to_detail_dto(
    proposal: &ProposalProjection,
) -> Result<VenearProposalDetailDto, ApiErrorResponse> {
    Ok(VenearProposalDetailDto {
        proposal_id: proposal.proposal_id,
        contract_id: proposal.contract_id.clone(),
        title: proposal_title(&proposal.metadata),
        description: proposal_description(&proposal.metadata),
        link: proposal_link(&proposal.metadata),
        voting_options: proposal.voting_options.clone(),
        status: proposal.status.clone(),
        proposer_id: proposal.proposer_id.clone(),
        reviewer_id: proposal.reviewer_id.clone(),
        metadata: proposal.metadata.clone(),
        snapshot: proposal_snapshot_dto(proposal),
        milestones: VenearProposalMilestonesDto {
            created_at: timestamp_from_ns_opt(Some(proposal.created_at_ns))?.unwrap_or_default(),
            approved_at: timestamp_from_ns_opt(proposal.approved_at_ns)?,
            rejected_at: timestamp_from_ns_opt(proposal.rejected_at_ns)?,
            voting_start_at: timestamp_from_ns_opt(proposal.voting_start_at_ns)?,
            voting_end_at: timestamp_from_ns_opt(
                proposal
                    .voting_start_at_ns
                    .zip(proposal.voting_duration_ns)
                    .map(|(start, duration)| start.saturating_add(duration)),
            )?,
        },
        tally: proposal.tally.clone(),
        turnout_voter_count: proposal.turnout_voter_count,
        turnout_voting_power: amount_from_raw(proposal.turnout_voting_power_raw.clone()),
        latest_event_type: proposal.latest_event_type.clone(),
        refs: proposal.latest_refs.clone(),
    })
}

fn proposal_snapshot_dto(proposal: &ProposalProjection) -> Option<VenearProposalSnapshotDto> {
    proposal
        .snapshot_present
        .then(|| VenearProposalSnapshotDto {
            block_height: proposal.snapshot_block_height,
            root: proposal.snapshot_root.clone(),
            total_venear: amount_from_raw(proposal.snapshot_total_venear_raw.clone()),
        })
}

pub(crate) fn proposal_timeline_events(
    proposal: &ProposalProjection,
) -> Result<Vec<VenearProposalTimelineEventDto>, ApiErrorResponse> {
    let mut events = vec![VenearProposalTimelineEventDto {
        event_type: "create_proposal".to_string(),
        proposal_id: proposal.proposal_id,
        actor_account_id: Some(proposal.proposer_id.clone()),
        status_after_event: "created".to_string(),
        refs: proposal.create_refs.clone(),
        payload: proposal.metadata.clone(),
    }];

    if let Some(refs) = proposal.approve_requested_refs.clone() {
        events.push(VenearProposalTimelineEventDto {
            event_type: "proposal_approve_requested".to_string(),
            proposal_id: proposal.proposal_id,
            actor_account_id: proposal.reviewer_id.clone(),
            status_after_event: "created".to_string(),
            refs,
            payload: proposal
                .approve_requested_payload
                .clone()
                .unwrap_or(JsonValue::Null),
        });
    }

    if let Some(refs) = proposal.approve_finalized_refs.clone() {
        events.push(VenearProposalTimelineEventDto {
            event_type: "proposal_approve_finalized".to_string(),
            proposal_id: proposal.proposal_id,
            actor_account_id: proposal.reviewer_id.clone(),
            status_after_event: proposal.status.clone(),
            refs,
            payload: proposal
                .approve_finalized_payload
                .clone()
                .unwrap_or(JsonValue::Null),
        });
    }

    if let Some(refs) = proposal.reject_refs.clone() {
        events.push(VenearProposalTimelineEventDto {
            event_type: "proposal_reject".to_string(),
            proposal_id: proposal.proposal_id,
            actor_account_id: proposal.reviewer_id.clone(),
            status_after_event: "rejected".to_string(),
            refs,
            payload: proposal.reject_payload.clone().unwrap_or(JsonValue::Null),
        });
    }

    Ok(events)
}

fn vote_projection_to_vote_dto(
    event: &VoteHistoryProjection,
    options: &[String],
) -> VenearProposalVoteV2Dto {
    VenearProposalVoteV2Dto {
        proposal_id: event.proposal_id,
        voter_account_id: event.voter_account_id.clone(),
        vote_option: event.vote_option.unwrap_or_default(),
        vote_label: event
            .vote_option
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| options.get(index).cloned()),
        voting_power: amount_from_raw(event.voting_power_raw.clone()),
        voted_at: event.refs.block_timestamp.clone(),
        refs: event.refs.clone(),
    }
}

pub(crate) fn vote_projection_to_history_dto(
    event: &VoteHistoryProjection,
    options: &[String],
) -> VenearProposalVoteHistoryEventDto {
    VenearProposalVoteHistoryEventDto {
        proposal_id: event.proposal_id,
        voter_account_id: event.voter_account_id.clone(),
        event_type: event.event_type.clone(),
        vote_option: event.vote_option,
        vote_label: event
            .vote_option
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| options.get(index).cloned()),
        voting_power: amount_from_raw(event.voting_power_raw.clone()),
        effective_after_event: event.effective_after_event,
        refs: event.refs.clone(),
    }
}

fn paginate_with_offset(
    mut data: Vec<VenearProposalSummaryV2Dto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearProposalListResponse, ApiErrorResponse> {
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
    Ok(VenearProposalListResponse {
        data,
        next_cursor,
        has_more,
    })
}

fn paginate_with_offset_timeline(
    mut data: Vec<VenearProposalTimelineEventDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearProposalTimelineListResponse, ApiErrorResponse> {
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
    Ok(VenearProposalTimelineListResponse {
        data,
        next_cursor,
        has_more,
    })
}

fn paginate_with_offset_votes(
    mut data: Vec<VenearProposalVoteV2Dto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearProposalVoteListResponse, ApiErrorResponse> {
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
    Ok(VenearProposalVoteListResponse {
        data,
        next_cursor,
        has_more,
    })
}

fn paginate_with_offset_vote_history(
    mut data: Vec<VenearProposalVoteHistoryEventDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearProposalVoteHistoryListResponse, ApiErrorResponse> {
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
    Ok(VenearProposalVoteHistoryListResponse {
        data,
        next_cursor,
        has_more,
    })
}
