use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{HashMap, HashSet};

use super::{
    admin::load_admin_action_projections,
    config::load_config_snapshots,
    delegations::{load_active_delegations, load_delegation_history},
    lockups::load_lockup_projections,
    proposals::{effective_vote_events, load_proposal_projections, load_vote_history},
    sql::{
        amount_from_optional_raw, amount_from_raw, chain_ref, decode_cursor, encode_cursor,
        is_success_status, json_value_to_string, load_canonical_actions, normalized_text_filter,
        require_db, resolve_limit, timestamp_from_ns_opt,
    },
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OffsetCursor {
    offset: usize,
}

#[derive(Debug, Clone)]
struct VotingPowerMutationProjection {
    account_id: String,
    reason: String,
    event_type: String,
    proposal_id: Option<i64>,
    delta_raw: String,
    effective_after_raw: String,
    refs: VenearChainRefDto,
}

#[derive(Debug, Clone)]
pub(crate) struct RewardEventProjection {
    pub account_id: String,
    pub event_type: String,
    pub amount_raw: Option<String>,
    pub refs: VenearChainRefDto,
    pub payload: JsonValue,
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/accounts",
    params(VenearAccountListQuery),
    tag = "venear",
    operation_id = "listVenearAccountsCanonical",
    responses(
        (status = 200, description = "Account-centric veNEAR summaries derived from canonical NEAR actions", body = VenearAccountListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_accounts(
    State(app_state): State<AppState>,
    Query(query): Query<VenearAccountListQuery>,
) -> Result<Json<VenearAccountListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let q_filter =
        normalized_text_filter(query.q.as_deref()).map(|value| value.to_ascii_lowercase());
    let role_filter =
        normalized_text_filter(query.role.as_deref()).map(|value| value.to_ascii_lowercase());
    let min_power = query
        .min_effective_voting_power_raw
        .as_deref()
        .unwrap_or("0")
        .parse::<i128>()
        .unwrap_or(0);

    let mut accounts = load_account_projections(db.as_ref()).await?;
    accounts.retain(|account| {
        if let Some(q) = q_filter.as_deref() {
            if !account.account_id.to_ascii_lowercase().contains(q) {
                return false;
            }
        }
        if let Some(role) = role_filter.as_deref() {
            if !account.roles.iter().any(|value| value == role) {
                return false;
            }
        }
        if let Some(flag) = query.has_inbound_delegation {
            if account.delegation.has_inbound_delegation != flag {
                return false;
            }
        }
        if let Some(flag) = query.has_outbound_delegation {
            if account.delegation.has_outbound_delegation != flag {
                return false;
            }
        }
        if let Some(flag) = query.has_lockup {
            if account.lockup.is_some() != flag {
                return false;
            }
        }
        if account
            .effective_voting_power
            .raw
            .parse::<i128>()
            .unwrap_or(0)
            < min_power
        {
            return false;
        }
        true
    });
    accounts.sort_by(|left, right| {
        right
            .effective_voting_power
            .raw
            .parse::<i128>()
            .unwrap_or(0)
            .cmp(&left.effective_voting_power.raw.parse::<i128>().unwrap_or(0))
            .then_with(|| left.account_id.cmp(&right.account_id))
    });

    let total = accounts.len();
    let data = accounts
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .map(|account| VenearAccountSummaryDto {
            account_id: account.account_id,
            roles: account.roles,
            effective_voting_power: account.effective_voting_power,
            own_voting_power: account.own_voting_power,
            delegated_voting_power: account.delegated_voting_power,
            delegated_to_account_id: account.delegation.delegated_to_account_id,
            inbound_delegator_count: account.delegation.inbound_delegator_count,
            has_lockup: account.lockup.is_some(),
            latest_activity_at: account.latest_activity_at,
        })
        .collect::<Vec<_>>();

    paginate_account_summaries(data, cursor.offset, limit, total).map(Json)
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/accounts/{account_id}",
    tag = "venear",
    operation_id = "getVenearAccountCanonical",
    responses(
        (status = 200, description = "Account detail derived from canonical NEAR actions", body = VenearAccountDetailDto),
        (status = 404, description = "Account not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_account(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
) -> Result<Json<VenearAccountDetailDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let account = load_account_projections(db.as_ref())
        .await?
        .into_iter()
        .find(|account| account.account_id == account_id)
        .ok_or_else(|| ApiErrorResponse::not_found("Account not found"))?;

    Ok(Json(account))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/accounts/{account_id}/proposals",
    params(VenearAccountProposalsQuery),
    tag = "venear",
    operation_id = "listVenearAccountProposalsCanonical",
    responses(
        (status = 200, description = "Proposal summaries related to one account", body = VenearAccountProposalListResponse),
        (status = 404, description = "Account not found", body = crate::errors::ApiError),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_account_proposals(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
    Query(query): Query<VenearAccountProposalsQuery>,
) -> Result<Json<VenearAccountProposalListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let role_filter =
        normalized_text_filter(query.role.as_deref()).map(|value| value.to_ascii_lowercase());
    let status_filter =
        normalized_text_filter(query.status.as_deref()).map(|value| value.to_ascii_lowercase());

    ensure_account_exists(db.as_ref(), &account_id).await?;
    let current_votes = effective_vote_events(load_vote_history(db.as_ref()).await?);
    let voted_proposals = current_votes
        .iter()
        .filter(|event| event.voter_account_id == account_id)
        .map(|event| event.proposal_id)
        .collect::<HashSet<_>>();

    let mut proposals = load_proposal_projections(db.as_ref()).await?;
    proposals.retain(|proposal| {
        let matches_role = match role_filter.as_deref() {
            Some("creator") => proposal.proposer_id == account_id,
            Some("reviewer") => proposal.reviewer_id.as_deref() == Some(account_id.as_str()),
            Some("voter") => voted_proposals.contains(&proposal.proposal_id),
            Some(_) => false,
            None => {
                proposal.proposer_id == account_id
                    || proposal.reviewer_id.as_deref() == Some(account_id.as_str())
                    || voted_proposals.contains(&proposal.proposal_id)
            }
        };
        if !matches_role {
            return false;
        }
        if let Some(status) = status_filter.as_deref() {
            if proposal.status != status {
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
        .map(super::proposals::projection_to_summary_dto)
        .collect::<Result<Vec<_>, _>>()?;

    paginate_account_proposals(data, cursor.offset, limit, total).map(Json)
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/accounts/{account_id}/votes",
    params(VenearAccountVotesQuery),
    tag = "venear",
    operation_id = "listVenearAccountVotesCanonical",
    responses(
        (status = 200, description = "Current effective votes cast by one account", body = VenearAccountVoteListResponse),
        (status = 404, description = "Account not found", body = crate::errors::ApiError),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_account_votes(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
    Query(query): Query<VenearAccountVotesQuery>,
) -> Result<Json<VenearAccountVoteListResponse>, ApiErrorResponse> {
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
        normalized_text_filter(query.status.as_deref()).map(|value| value.to_ascii_lowercase());

    ensure_account_exists(db.as_ref(), &account_id).await?;
    let proposals = load_proposal_projections(db.as_ref()).await?;
    let proposal_by_id = proposals
        .iter()
        .map(|proposal| (proposal.proposal_id, proposal))
        .collect::<HashMap<_, _>>();

    let mut votes = effective_vote_events(load_vote_history(db.as_ref()).await?)
        .into_iter()
        .filter(|event| event.voter_account_id == account_id)
        .filter_map(|event| {
            let proposal = proposal_by_id.get(&event.proposal_id)?;
            if let Some(status) = status_filter.as_deref() {
                if proposal.status != status {
                    return None;
                }
            }
            Some(VenearAccountVoteDto {
                proposal_id: proposal.proposal_id,
                proposal_title: proposal
                    .metadata
                    .get("title")
                    .and_then(json_value_to_string),
                vote_option: event.vote_option.unwrap_or_default(),
                vote_label: event
                    .vote_option
                    .and_then(|index| usize::try_from(index).ok())
                    .and_then(|index| proposal.voting_options.get(index).cloned()),
                voting_power: amount_from_raw(event.voting_power_raw),
                voted_at: event.refs.block_timestamp.clone(),
                refs: event.refs,
            })
        })
        .collect::<Vec<_>>();
    votes.sort_by(|left, right| {
        right
            .refs
            .block_height
            .cmp(&left.refs.block_height)
            .then_with(|| right.proposal_id.cmp(&left.proposal_id))
    });

    let total = votes.len();
    let data = votes
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .collect::<Vec<_>>();

    paginate_account_votes(data, cursor.offset, limit, total).map(Json)
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/accounts/{account_id}/vote_history",
    params(VenearAccountVoteHistoryQuery),
    tag = "venear",
    operation_id = "listVenearAccountVoteHistoryCanonical",
    responses(
        (status = 200, description = "Full successful vote history for one account", body = VenearAccountVoteHistoryListResponse),
        (status = 404, description = "Account not found", body = crate::errors::ApiError),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_account_vote_history(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
    Query(query): Query<VenearAccountVoteHistoryQuery>,
) -> Result<Json<VenearAccountVoteHistoryListResponse>, ApiErrorResponse> {
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

    ensure_account_exists(db.as_ref(), &account_id).await?;
    let proposals = load_proposal_projections(db.as_ref()).await?;
    let options_by_id = proposals
        .iter()
        .map(|proposal| (proposal.proposal_id, proposal.voting_options.clone()))
        .collect::<HashMap<_, _>>();

    let mut history = load_vote_history(db.as_ref()).await?;
    history.retain(|event| {
        if event.voter_account_id != account_id {
            return false;
        }
        if let Some(proposal_id) = query.proposal_id {
            if event.proposal_id != proposal_id {
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
        .map(|event| {
            let options = options_by_id
                .get(&event.proposal_id)
                .cloned()
                .unwrap_or_default();
            super::proposals::vote_projection_to_history_dto(&event, &options)
        })
        .collect::<Vec<_>>();

    paginate_account_vote_history(data, cursor.offset, limit, total).map(Json)
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/accounts/{account_id}/voting_power_history",
    params(VenearAccountVotingPowerHistoryQuery),
    tag = "venear",
    operation_id = "listVenearAccountVotingPowerHistoryCanonical",
    responses(
        (status = 200, description = "Derived voting power mutation timeline for one account", body = VenearAccountVotingPowerHistoryListResponse),
        (status = 404, description = "Account not found", body = crate::errors::ApiError),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_account_voting_power_history(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
    Query(query): Query<VenearAccountVotingPowerHistoryQuery>,
) -> Result<Json<VenearAccountVotingPowerHistoryListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.page.limit);
    let cursor = query
        .page
        .cursor
        .as_deref()
        .map(decode_cursor::<OffsetCursor>)
        .transpose()?
        .unwrap_or(OffsetCursor { offset: 0 });
    let reason_filter =
        normalized_text_filter(query.reason.as_deref()).map(|value| value.to_ascii_lowercase());

    ensure_account_exists(db.as_ref(), &account_id).await?;
    let mut history = load_voting_power_mutations(db.as_ref()).await?;
    history.retain(|event| {
        if event.account_id != account_id {
            return false;
        }
        if let Some(reason) = reason_filter.as_deref() {
            if event.reason != reason {
                return false;
            }
        }
        if let Some(proposal_id) = query.proposal_id {
            if event.proposal_id != Some(proposal_id) {
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
        .map(|event| VenearAccountVotingPowerEventV2Dto {
            account_id: event.account_id,
            reason: event.reason,
            event_type: event.event_type,
            proposal_id: event.proposal_id,
            delta: amount_from_raw(event.delta_raw),
            effective_voting_power_after: amount_from_raw(event.effective_after_raw),
            refs: event.refs,
        })
        .collect::<Vec<_>>();

    paginate_account_vp_history(data, cursor.offset, limit, total).map(Json)
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/accounts/{account_id}/rewards_events",
    params(VenearAccountRewardsEventsQuery),
    tag = "venear",
    operation_id = "listVenearAccountRewardsEventsCanonical",
    responses(
        (status = 200, description = "Reward and claim events scoped to one account", body = VenearAccountRewardsEventsListResponse),
        (status = 404, description = "Account not found", body = crate::errors::ApiError),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_account_rewards_events(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
    Query(query): Query<VenearAccountRewardsEventsQuery>,
) -> Result<Json<VenearAccountRewardsEventsListResponse>, ApiErrorResponse> {
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

    ensure_account_exists(db.as_ref(), &account_id).await?;
    let mut events = load_reward_events(db.as_ref()).await?;
    events.retain(|event| {
        if event.account_id != account_id {
            return false;
        }
        if let Some(filter) = event_type_filter.as_deref() {
            if event.event_type != filter {
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
            .then_with(|| right.refs.receipt_id.cmp(&left.refs.receipt_id))
    });

    let total = events.len();
    let data = events
        .into_iter()
        .skip(cursor.offset)
        .take((limit as usize) + 1)
        .map(|event| VenearRewardEventDto {
            account_id: event.account_id,
            event_type: event.event_type,
            amount: amount_from_optional_raw(event.amount_raw),
            refs: event.refs,
            payload: event.payload,
        })
        .collect::<Vec<_>>();

    paginate_account_rewards(data, cursor.offset, limit, total).map(Json)
}

pub(crate) async fn ensure_account_exists(
    db: &sea_orm::DatabaseConnection,
    account_id: &str,
) -> Result<(), ApiErrorResponse> {
    if load_account_projections(db)
        .await?
        .into_iter()
        .any(|account| account.account_id == account_id)
    {
        Ok(())
    } else {
        Err(ApiErrorResponse::not_found("Account not found"))
    }
}

async fn load_account_projections(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<VenearAccountDetailDto>, ApiErrorResponse> {
    let proposals = load_proposal_projections(db).await?;
    let vote_history = load_vote_history(db).await?;
    let effective_votes = effective_vote_events(vote_history.clone());
    let active_delegations = load_active_delegations(db).await?;
    let delegation_history = load_delegation_history(db).await?;
    let lockups = load_lockup_projections(db).await?;
    let config_snapshots = load_config_snapshots(db).await?;
    let admin_actions = load_admin_action_projections(db).await?;
    let reward_events = load_reward_events(db).await?;
    let voting_power_mutations = load_voting_power_mutations(db).await?;

    let mut account_ids = HashSet::<String>::new();
    for proposal in &proposals {
        account_ids.insert(proposal.proposer_id.clone());
        if let Some(reviewer_id) = proposal.reviewer_id.clone() {
            account_ids.insert(reviewer_id);
        }
    }
    for event in &effective_votes {
        account_ids.insert(event.voter_account_id.clone());
    }
    for edge in &active_delegations {
        account_ids.insert(edge.delegator_account_id.clone());
        account_ids.insert(edge.delegatee_account_id.clone());
    }
    for event in &delegation_history {
        account_ids.insert(event.delegator_account_id.clone());
        if let Some(delegatee) = event.delegatee_account_id.clone() {
            account_ids.insert(delegatee);
        }
    }
    for lockup in &lockups {
        account_ids.insert(lockup.owner_account_id.clone());
    }
    for snapshot in &config_snapshots {
        if let Some(owner) = snapshot.owner_account_id.clone() {
            account_ids.insert(owner);
        }
        if let Some(owner) = snapshot.proposed_new_owner_account_id.clone() {
            account_ids.insert(owner);
        }
        account_ids.extend(snapshot.guardians.clone());
        account_ids.extend(snapshot.reviewer_ids.clone());
        account_ids.extend(snapshot.lockup_code_deployers.clone());
    }
    for action in &admin_actions {
        if let Some(actor) = action.actor_account_id.clone() {
            account_ids.insert(actor);
        }
    }
    for reward in &reward_events {
        account_ids.insert(reward.account_id.clone());
    }
    for mutation in &voting_power_mutations {
        account_ids.insert(mutation.account_id.clone());
    }

    let inbound_by_account =
        active_delegations
            .iter()
            .fold(HashMap::<String, (i64, i128)>::new(), |mut acc, edge| {
                let entry = acc
                    .entry(edge.delegatee_account_id.clone())
                    .or_insert((0, 0));
                entry.0 += 1;
                entry.1 += edge.delegated_voting_power_raw.parse::<i128>().unwrap_or(0);
                acc
            });
    let outbound_by_account = active_delegations
        .iter()
        .map(|edge| {
            (
                edge.delegator_account_id.clone(),
                edge.delegatee_account_id.clone(),
            )
        })
        .collect::<HashMap<_, _>>();
    let lockup_by_owner = lockups
        .iter()
        .map(|lockup| (lockup.owner_account_id.clone(), lockup.clone()))
        .collect::<HashMap<_, _>>();
    let latest_ref_by_account = latest_refs_by_account(
        &proposals,
        &effective_votes,
        &active_delegations,
        &lockups,
        &admin_actions,
        &reward_events,
    );
    let effective_power_by_account = current_effective_power_by_account(&voting_power_mutations);
    let current_votes_count_by_account =
        effective_votes
            .iter()
            .fold(HashMap::<String, i64>::new(), |mut acc, event| {
                *acc.entry(event.voter_account_id.clone()).or_default() += 1;
                acc
            });
    let proposals_created_count_by_account =
        proposals
            .iter()
            .fold(HashMap::<String, i64>::new(), |mut acc, proposal| {
                *acc.entry(proposal.proposer_id.clone()).or_default() += 1;
                acc
            });
    let proposals_reviewed_count_by_account = proposals
        .iter()
        .filter_map(|proposal| proposal.reviewer_id.clone())
        .fold(HashMap::<String, i64>::new(), |mut acc, reviewer_id| {
            *acc.entry(reviewer_id).or_default() += 1;
            acc
        });
    let vote_events_count_by_account =
        vote_history
            .iter()
            .fold(HashMap::<String, i64>::new(), |mut acc, event| {
                *acc.entry(event.voter_account_id.clone()).or_default() += 1;
                acc
            });

    let mut accounts = Vec::new();
    for account_id in account_ids {
        let effective_raw = effective_power_by_account
            .get(&account_id)
            .cloned()
            .unwrap_or_else(|| "0".to_string());
        let inbound = inbound_by_account
            .get(&account_id)
            .cloned()
            .unwrap_or((0, 0));
        let delegated_voting_power_raw = inbound.1.to_string();
        let own_raw = effective_raw
            .parse::<i128>()
            .unwrap_or(0)
            .saturating_sub(inbound.1)
            .to_string();
        let current_votes_count = current_votes_count_by_account
            .get(&account_id)
            .copied()
            .unwrap_or_default();
        let proposals_created_count = proposals_created_count_by_account
            .get(&account_id)
            .copied()
            .unwrap_or_default();
        let proposals_reviewed_count = proposals_reviewed_count_by_account
            .get(&account_id)
            .copied()
            .unwrap_or_default();
        let vote_events_count = vote_events_count_by_account
            .get(&account_id)
            .copied()
            .unwrap_or_default();

        let lockup_summary =
            lockup_by_owner
                .get(&account_id)
                .map(|lockup| VenearAccountLockupSummaryDto {
                    lockup_account_id: lockup.lockup_account_id.clone(),
                    status: lockup.status.clone(),
                    locked_near_balance: amount_from_raw(lockup.locked_near_balance_raw.clone()),
                    last_updated_at: lockup.latest_refs.block_timestamp.clone(),
                });
        let roles = derive_roles(
            &account_id,
            &proposals,
            &effective_votes,
            &active_delegations,
            &lockup_by_owner,
            &admin_actions,
            &config_snapshots,
        );
        let latest_refs = latest_ref_by_account.get(&account_id).cloned();
        let latest_activity_at = latest_refs
            .as_ref()
            .map(|refs| refs.block_timestamp.clone());
        let governance_state = if current_votes_count > 0
            || proposals_created_count > 0
            || proposals_reviewed_count > 0
        {
            Some(VenearAccountGovernanceStateDto {
                current_effective_votes_count: current_votes_count,
                proposals_created_count,
                proposals_reviewed_count,
            })
        } else {
            None
        };

        accounts.push(VenearAccountDetailDto {
            account_id: account_id.clone(),
            roles,
            effective_voting_power: amount_from_raw(effective_raw),
            own_voting_power: amount_from_raw(own_raw),
            delegated_voting_power: amount_from_raw(delegated_voting_power_raw),
            delegation: VenearAccountDelegationSummaryDto {
                delegated_to_account_id: outbound_by_account.get(&account_id).cloned(),
                inbound_delegator_count: inbound.0,
                has_inbound_delegation: inbound.0 > 0,
                has_outbound_delegation: outbound_by_account.contains_key(&account_id),
            },
            lockup: lockup_summary,
            participation: VenearAccountParticipationDto {
                proposals_created_count,
                proposals_reviewed_count,
                vote_events_count,
                current_effective_votes_count: current_votes_count,
            },
            latest_activity_at,
            latest_refs,
            governance_state,
        });
    }

    Ok(accounts)
}

fn derive_roles(
    account_id: &str,
    proposals: &[super::proposals::ProposalProjection],
    votes: &[super::proposals::VoteHistoryProjection],
    active_delegations: &[super::delegations::DelegationEdgeProjection],
    lockups: &HashMap<String, super::lockups::LockupProjection>,
    admin_actions: &[super::admin::AdminActionProjection],
    config_snapshots: &[super::config::ConfigSnapshotProjection],
) -> Vec<String> {
    let mut roles = HashSet::new();
    if proposals
        .iter()
        .any(|proposal| proposal.proposer_id == account_id)
    {
        roles.insert("proposer".to_string());
    }
    if proposals
        .iter()
        .any(|proposal| proposal.reviewer_id.as_deref() == Some(account_id))
    {
        roles.insert("reviewer".to_string());
    }
    if votes
        .iter()
        .any(|event| event.voter_account_id == account_id)
    {
        roles.insert("voter".to_string());
    }
    if active_delegations
        .iter()
        .any(|edge| edge.delegator_account_id == account_id)
    {
        roles.insert("delegator".to_string());
    }
    if active_delegations
        .iter()
        .any(|edge| edge.delegatee_account_id == account_id)
    {
        roles.insert("delegatee".to_string());
    }
    if lockups.contains_key(account_id) {
        roles.insert("lockup_owner".to_string());
    }
    if admin_actions
        .iter()
        .any(|action| action.actor_account_id.as_deref() == Some(account_id))
    {
        roles.insert("admin".to_string());
    }
    if config_snapshots.iter().any(|snapshot| {
        snapshot.owner_account_id.as_deref() == Some(account_id)
            || snapshot.proposed_new_owner_account_id.as_deref() == Some(account_id)
            || snapshot.guardians.iter().any(|value| value == account_id)
            || snapshot
                .reviewer_ids
                .iter()
                .any(|value| value == account_id)
            || snapshot
                .lockup_code_deployers
                .iter()
                .any(|value| value == account_id)
    }) {
        roles.insert("config_actor".to_string());
    }

    let mut roles = roles.into_iter().collect::<Vec<_>>();
    roles.sort();
    roles
}

fn latest_refs_by_account(
    proposals: &[super::proposals::ProposalProjection],
    votes: &[super::proposals::VoteHistoryProjection],
    delegations: &[super::delegations::DelegationEdgeProjection],
    lockups: &[super::lockups::LockupProjection],
    admin_actions: &[super::admin::AdminActionProjection],
    reward_events: &[RewardEventProjection],
) -> HashMap<String, VenearChainRefDto> {
    let mut latest = HashMap::<String, VenearChainRefDto>::new();
    let mut upsert = |account_id: &str, refs: &VenearChainRefDto| {
        let replace = latest
            .get(account_id)
            .map(|current| {
                refs.block_height > current.block_height
                    || (refs.block_height == current.block_height
                        && refs.receipt_id > current.receipt_id)
            })
            .unwrap_or(true);
        if replace {
            latest.insert(account_id.to_string(), refs.clone());
        }
    };

    for proposal in proposals {
        upsert(&proposal.proposer_id, &proposal.latest_refs);
        if let Some(reviewer_id) = proposal.reviewer_id.as_deref() {
            upsert(reviewer_id, &proposal.latest_refs);
        }
    }
    for vote in votes {
        upsert(&vote.voter_account_id, &vote.refs);
    }
    for delegation in delegations {
        upsert(&delegation.delegator_account_id, &delegation.refs);
        upsert(&delegation.delegatee_account_id, &delegation.refs);
    }
    for lockup in lockups {
        upsert(&lockup.owner_account_id, &lockup.latest_refs);
    }
    for action in admin_actions {
        if let Some(actor) = action.actor_account_id.as_deref() {
            upsert(actor, &action.refs);
        }
    }
    for reward in reward_events {
        upsert(&reward.account_id, &reward.refs);
    }

    latest
}

fn current_effective_power_by_account(
    mutations: &[VotingPowerMutationProjection],
) -> HashMap<String, String> {
    let mut latest = HashMap::<String, String>::new();
    for mutation in mutations {
        latest.insert(
            mutation.account_id.clone(),
            mutation.effective_after_raw.clone(),
        );
    }
    latest
}

async fn load_voting_power_mutations(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<VotingPowerMutationProjection>, ApiErrorResponse> {
    let actions = load_canonical_actions(
        db,
        &[
            "storage_deposit",
            "on_lockup_update",
            "delegate_all",
            "undelegate",
            "lock_near",
            "begin_unlock_near",
            "end_unlock_near",
            "claim",
            "vote",
        ],
    )
    .await?;
    let vote_history = load_vote_history(db).await?;

    let mut ledger_events = Vec::<(
        String,
        String,
        String,
        Option<i64>,
        String,
        VenearChainRefDto,
    )>::new();
    let mut seen_balance_logs = HashSet::<(String, i64)>::new();
    for action in actions {
        if !is_success_status(action.status.as_deref()) {
            continue;
        }
        let base_refs = chain_ref(
            action.contract_id.clone(),
            action.transaction_hash.clone(),
            action.receipt_id.clone(),
            action.execution_outcome_id.clone(),
            action.block_height,
            timestamp_from_ns_opt(Some(action.block_timestamp))?
                .unwrap_or_else(|| action.block_timestamp.to_string()),
        );
        for log in &action.logs {
            if !matches!(log.event_type.as_str(), "ft_mint" | "ft_burn") {
                continue;
            }
            if !seen_balance_logs.insert((action.execution_outcome_id.clone(), log.log_index)) {
                continue;
            }
            let account_id = log
                .payload
                .get("data")
                .and_then(|value| value.get(0))
                .and_then(|value| value.get("owner_id"))
                .and_then(json_value_to_string)
                .unwrap_or_else(|| action.predecessor_id.clone());
            let amount_raw = log
                .payload
                .get("data")
                .and_then(|value| value.get(0))
                .and_then(|value| value.get("amount"))
                .and_then(json_value_to_string)
                .unwrap_or_else(|| "0".to_string());
            let delta_raw = if log.event_type == "ft_burn" {
                format!("-{amount_raw}")
            } else {
                amount_raw.clone()
            };
            let reason = match (action.method_name.as_str(), log.event_type.as_str()) {
                ("delegate_all", "ft_burn") => "delegation_given",
                ("delegate_all", "ft_mint") => "delegation_received",
                ("undelegate", _) => "delegation_removed",
                ("claim", _) => "reward_claimed",
                _ if log.event_type == "ft_burn" => "balance_decrease",
                _ => "balance_increase",
            };
            ledger_events.push((
                account_id,
                reason.to_string(),
                log.event_type.clone(),
                None,
                delta_raw,
                base_refs.clone(),
            ));
        }
    }

    for event in vote_history {
        let reason = if event.event_type == "add_vote" {
            "vote_cast"
        } else {
            "vote_changed"
        };
        ledger_events.push((
            event.voter_account_id,
            reason.to_string(),
            event.event_type,
            Some(event.proposal_id),
            "0".to_string(),
            event.refs,
        ));
    }

    ledger_events.sort_by(|left, right| {
        left.5
            .block_height
            .cmp(&right.5.block_height)
            .then_with(|| left.5.receipt_id.cmp(&right.5.receipt_id))
    });

    let mut running = HashMap::<String, i128>::new();
    let mut projections = Vec::new();
    for (account_id, reason, event_type, proposal_id, delta_raw, refs) in ledger_events {
        let entry = running.entry(account_id.clone()).or_insert(0);
        *entry += delta_raw.parse::<i128>().unwrap_or(0);
        projections.push(VotingPowerMutationProjection {
            account_id,
            reason,
            event_type,
            proposal_id,
            delta_raw,
            effective_after_raw: entry.to_string(),
            refs,
        });
    }

    Ok(projections)
}

pub(crate) async fn load_reward_events(
    db: &sea_orm::DatabaseConnection,
) -> Result<Vec<RewardEventProjection>, ApiErrorResponse> {
    let actions = load_canonical_actions(db, &["claim"]).await?;
    Ok(actions
        .into_iter()
        .filter(|action| is_success_status(action.status.as_deref()))
        .map(|action| {
            let amount_raw = action.logs.iter().find_map(|log| {
                if log.event_type.contains("claim") {
                    log.payload
                        .get("data")
                        .and_then(|value| value.get(0))
                        .and_then(|value| value.get("amount"))
                        .and_then(json_value_to_string)
                        .or_else(|| {
                            log.payload
                                .get("data")
                                .and_then(|value| value.get("claimed"))
                                .and_then(json_value_to_string)
                        })
                } else {
                    None
                }
            });

            RewardEventProjection {
                account_id: action.predecessor_id.clone(),
                event_type: action
                    .logs
                    .iter()
                    .find_map(|log| {
                        if log.event_type.contains("claim") {
                            Some(log.event_type.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "claim".to_string()),
                amount_raw,
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
            }
        })
        .collect())
}

fn paginate_account_summaries(
    mut data: Vec<VenearAccountSummaryDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearAccountListResponse, ApiErrorResponse> {
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
    Ok(VenearAccountListResponse {
        data,
        next_cursor,
        has_more,
    })
}

fn paginate_account_proposals(
    mut data: Vec<VenearProposalSummaryV2Dto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearAccountProposalListResponse, ApiErrorResponse> {
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
    Ok(VenearAccountProposalListResponse {
        data,
        next_cursor,
        has_more,
    })
}

fn paginate_account_votes(
    mut data: Vec<VenearAccountVoteDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearAccountVoteListResponse, ApiErrorResponse> {
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
    Ok(VenearAccountVoteListResponse {
        data,
        next_cursor,
        has_more,
    })
}

fn paginate_account_vote_history(
    mut data: Vec<VenearProposalVoteHistoryEventDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearAccountVoteHistoryListResponse, ApiErrorResponse> {
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
    Ok(VenearAccountVoteHistoryListResponse {
        data,
        next_cursor,
        has_more,
    })
}

fn paginate_account_vp_history(
    mut data: Vec<VenearAccountVotingPowerEventV2Dto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearAccountVotingPowerHistoryListResponse, ApiErrorResponse> {
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
    Ok(VenearAccountVotingPowerHistoryListResponse {
        data,
        next_cursor,
        has_more,
    })
}

fn paginate_account_rewards(
    mut data: Vec<VenearRewardEventDto>,
    offset: usize,
    limit: u64,
    total: usize,
) -> Result<VenearAccountRewardsEventsListResponse, ApiErrorResponse> {
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
    Ok(VenearAccountRewardsEventsListResponse {
        data,
        next_cursor,
        has_more,
    })
}
