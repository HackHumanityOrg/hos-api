//! veNEAR governance read API handlers.

use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, NaiveDateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, Value,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Serialize, Deserialize)]
struct ProposalCursor {
    proposal_id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct HeightIdCursor {
    block_height: i64,
    id: i64,
}

#[derive(Debug, Serialize, Deserialize)]
struct StringCursor {
    key: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/proposals",
    summary = "List latest state per proposal",
    description = "Returns one latest-event snapshot per proposal from the indexed proposal event stream, ordered by proposal id descending with cursor pagination.",
    params(CursorPaginationQuery),
    tag = "venear",
    operation_id = "listVenearProposals",
    responses(
        (status = 200, description = "Latest event state for each proposal", body = VenearProposalsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_proposals(
    State(app_state): State<AppState>,
    Query(query): Query<CursorPaginationQuery>,
) -> Result<Json<VenearProposalsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.limit);

    let proposal_cursor = match query.cursor.as_deref() {
        Some(cursor) => Some(decode_cursor::<ProposalCursor>(cursor)?.proposal_id),
        None => None,
    };

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT DISTINCT ON (proposal_id)
                id,
                proposal_id,
                event_type,
                account_id,
                proposer_id,
                reviewer_id,
                voting_start_time_sec,
                snapshot_block_height,
                snapshot_root,
                snapshot_total_venear,
                metadata,
                payload,
                block_height,
                block_timestamp,
                shard_id,
                receipt_id,
                execution_outcome_id,
                predecessor_account_id,
                contract_id,
                log_index,
                data_index,
                created_at
            FROM near.near_venear_proposal_events
            WHERE ($1::bigint IS NULL OR proposal_id < $1::bigint)
            ORDER BY proposal_id DESC, block_height DESC, id DESC
            LIMIT $2
            ",
            vec![
                proposal_cursor.into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(to_latest_proposal_dto)
        .collect::<Result<Vec<_>, _>>()?;

    let has_more = data.len() > limit as usize;
    if has_more {
        data.pop();
    }

    let next_cursor = data
        .last()
        .map(|row| {
            encode_cursor(&ProposalCursor {
                proposal_id: row.proposal_id,
            })
        })
        .transpose()?;

    Ok(Json(VenearProposalsListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/proposals/{proposal_id}/events",
    summary = "List all events for a proposal",
    description = "Returns the full indexed event stream for a proposal, ordered by block height and row id descending with cursor pagination.",
    params(
        ("proposal_id" = i64, Path, description = "Proposal id"),
        CursorPaginationQuery
    ),
    tag = "venear",
    operation_id = "listVenearProposalEvents",
    responses(
        (status = 200, description = "Proposal event rows", body = VenearProposalEventsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Proposal not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_proposal_events(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<i64>,
    Query(query): Query<CursorPaginationQuery>,
) -> Result<Json<VenearProposalEventsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_proposal_exists(db.as_ref(), proposal_id).await?;
    let limit = resolve_limit(query.limit);

    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<HeightIdCursor>)
        .transpose()?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                id,
                proposal_id,
                event_type,
                account_id,
                proposer_id,
                reviewer_id,
                voting_start_time_sec,
                snapshot_block_height,
                snapshot_root,
                snapshot_total_venear,
                metadata,
                payload,
                block_height,
                block_timestamp,
                shard_id,
                receipt_id,
                execution_outcome_id,
                predecessor_account_id,
                contract_id,
                log_index,
                data_index,
                created_at
            FROM near.near_venear_proposal_events
            WHERE proposal_id = $1
              AND (
                $2::bigint IS NULL
                OR block_height < $2::bigint
                OR (block_height = $2::bigint AND id < $3::bigint)
              )
            ORDER BY block_height DESC, id DESC
            LIMIT $4
            ",
            vec![
                proposal_id.into(),
                cursor.as_ref().map(|value| value.block_height).into(),
                cursor.as_ref().map(|value| value.id).into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(to_proposal_event_dto)
        .collect::<Result<Vec<_>, _>>()?;

    let has_more = data.len() > limit as usize;
    if has_more {
        data.pop();
    }

    let next_cursor = data
        .last()
        .map(|row| {
            encode_cursor(&HeightIdCursor {
                block_height: row.block_height,
                id: row.id,
            })
        })
        .transpose()?;

    Ok(Json(VenearProposalEventsListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/proposals/{proposal_id}/votes",
    summary = "List votes for a proposal",
    description = "Returns indexed vote rows for a proposal, ordered by block height and row id descending with cursor pagination.",
    params(
        ("proposal_id" = i64, Path, description = "Proposal id"),
        CursorPaginationQuery
    ),
    tag = "venear",
    operation_id = "listVenearProposalVotes",
    responses(
        (status = 200, description = "Proposal votes", body = VenearVotesListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Proposal not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_proposal_votes(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<i64>,
    Query(query): Query<CursorPaginationQuery>,
) -> Result<Json<VenearVotesListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_proposal_exists(db.as_ref(), proposal_id).await?;
    let limit = resolve_limit(query.limit);

    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<HeightIdCursor>)
        .transpose()?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                id,
                proposal_id,
                account_id,
                vote,
                account_balance,
                event_type,
                payload,
                block_height,
                block_timestamp,
                shard_id,
                receipt_id,
                execution_outcome_id,
                predecessor_account_id,
                contract_id,
                log_index,
                data_index,
                created_at
            FROM near.near_venear_votes
            WHERE proposal_id = $1
              AND (
                $2::bigint IS NULL
                OR block_height < $2::bigint
                OR (block_height = $2::bigint AND id < $3::bigint)
              )
            ORDER BY block_height DESC, id DESC
            LIMIT $4
            ",
            vec![
                proposal_id.into(),
                cursor.as_ref().map(|value| value.block_height).into(),
                cursor.as_ref().map(|value| value.id).into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(to_vote_dto)
        .collect::<Result<Vec<_>, _>>()?;

    let has_more = data.len() > limit as usize;
    if has_more {
        data.pop();
    }

    let next_cursor = data
        .last()
        .map(|row| {
            encode_cursor(&HeightIdCursor {
                block_height: row.block_height,
                id: row.id,
            })
        })
        .transpose()?;

    Ok(Json(VenearVotesListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/accounts/{account_id}/balances",
    summary = "List veNEAR balances for an account",
    description = "Returns balance mutation rows for a governance account, including deltas and totals, ordered by block height and row id descending.",
    params(
        ("account_id" = String, Path, description = "NEAR account id"),
        CursorPaginationQuery
    ),
    tag = "venear",
    operation_id = "listVenearAccountBalances",
    responses(
        (status = 200, description = "veNEAR balance rows for account", body = VenearAccountBalancesListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Account not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_account_balances(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
    Query(query): Query<CursorPaginationQuery>,
) -> Result<Json<VenearAccountBalancesListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_account_balance_exists(db.as_ref(), &account_id).await?;
    let limit = resolve_limit(query.limit);

    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<HeightIdCursor>)
        .transpose()?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                id,
                account_id,
                event_type,
                delta_amount,
                total_balance,
                locked_near_balance,
                lockup_update_nonce,
                lockup_version,
                block_height,
                block_timestamp,
                shard_id,
                receipt_id,
                execution_outcome_id,
                predecessor_account_id,
                contract_id,
                log_index,
                data_index,
                payload,
                created_at
            FROM near.near_venear_account_balances
            WHERE account_id = $1
              AND (
                $2::bigint IS NULL
                OR block_height < $2::bigint
                OR (block_height = $2::bigint AND id < $3::bigint)
              )
            ORDER BY block_height DESC, id DESC
            LIMIT $4
            ",
            vec![
                account_id.into(),
                cursor.as_ref().map(|value| value.block_height).into(),
                cursor.as_ref().map(|value| value.id).into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(to_account_balance_dto)
        .collect::<Result<Vec<_>, _>>()?;

    let has_more = data.len() > limit as usize;
    if has_more {
        data.pop();
    }

    let next_cursor = data
        .last()
        .map(|row| {
            encode_cursor(&HeightIdCursor {
                block_height: row.block_height,
                id: row.id,
            })
        })
        .transpose()?;

    Ok(Json(VenearAccountBalancesListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/accounts/{account_id}/delegations",
    summary = "List veNEAR delegation actions for an account",
    description = "Returns delegation action rows where the account appears as delegator or delegatee, ordered by block height and row id descending.",
    params(
        ("account_id" = String, Path, description = "NEAR account id"),
        VenearDelegationEventsQuery
    ),
    tag = "venear",
    operation_id = "listVenearAccountDelegations",
    responses(
        (status = 200, description = "veNEAR delegation rows for account", body = VenearDelegationEventsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Account not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_account_delegations(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
    Query(query): Query<VenearDelegationEventsQuery>,
) -> Result<Json<VenearDelegationEventsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_delegation_account_exists(db.as_ref(), &account_id).await?;
    let limit = resolve_limit(query.limit);

    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<HeightIdCursor>)
        .transpose()?;

    let event_type = query
        .event_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                id,
                delegator_account_id,
                delegatee_account_id,
                event_type,
                action_source,
                action_index,
                args_json,
                args_base64,
                attached_deposit,
                transaction_hash,
                receipt_id,
                execution_outcome_id,
                success,
                block_height,
                block_timestamp,
                shard_id,
                contract_id,
                metadata,
                created_at
            FROM near.near_venear_delegation_events
            WHERE (delegator_account_id = $1 OR delegatee_account_id = $1)
              AND ($2::text IS NULL OR event_type = $2)
              AND ($3::boolean IS NULL OR success = $3::boolean)
              AND (
                $4::bigint IS NULL
                OR block_height < $4::bigint
                OR (block_height = $4::bigint AND id < $5::bigint)
              )
            ORDER BY block_height DESC, id DESC
            LIMIT $6
            ",
            vec![
                account_id.into(),
                event_type.into(),
                query.success.into(),
                cursor.as_ref().map(|value| value.block_height).into(),
                cursor.as_ref().map(|value| value.id).into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(to_delegation_event_dto)
        .collect::<Result<Vec<_>, _>>()?;

    let has_more = data.len() > limit as usize;
    if has_more {
        data.pop();
    }

    let next_cursor = data
        .last()
        .map(|row| {
            encode_cursor(&HeightIdCursor {
                block_height: row.block_height,
                id: row.id,
            })
        })
        .transpose()?;

    Ok(Json(VenearDelegationEventsListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/lockups",
    summary = "List discovered lockup accounts",
    description = "Returns discovered veNEAR lockup accounts with lifecycle metadata. Supports owner and active-state filters and cursor pagination by lockup account id.",
    params(VenearLockupsQuery),
    tag = "venear",
    operation_id = "listVenearLockups",
    responses(
        (status = 200, description = "Known veNEAR lockups", body = VenearLockupsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_lockups(
    State(app_state): State<AppState>,
    Query(query): Query<VenearLockupsQuery>,
) -> Result<Json<VenearLockupsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.limit);

    let owner_account_id = query
        .owner_account_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let lockup_cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<StringCursor>)
        .transpose()?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                lockup_account_id,
                owner_account_id,
                lockup_version,
                discovery_source,
                discovered_at_height,
                discovered_at_timestamp,
                last_seen_height,
                last_seen_timestamp,
                is_active,
                deleted_at_height,
                deleted_at_timestamp,
                metadata
            FROM near.near_venear_lockups
            WHERE ($1::text IS NULL OR owner_account_id = $1::text)
              AND ($2::boolean IS NULL OR is_active = $2::boolean)
              AND ($3::text IS NULL OR lockup_account_id > $3::text)
            ORDER BY lockup_account_id ASC
            LIMIT $4
            ",
            vec![
                owner_account_id.into(),
                query.is_active.into(),
                lockup_cursor.as_ref().map(|value| value.key.clone()).into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(to_lockup_dto)
        .collect::<Result<Vec<_>, _>>()?;

    let has_more = data.len() > limit as usize;
    if has_more {
        data.pop();
    }

    let next_cursor = data
        .last()
        .map(|row| {
            encode_cursor(&StringCursor {
                key: row.lockup_account_id.clone(),
            })
        })
        .transpose()?;

    Ok(Json(VenearLockupsListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/lockups/{lockup_account_id}/actions",
    summary = "List actions for a lockup account",
    description = "Returns indexed lockup-contract actions for a lockup account, with optional method-name and success filters, ordered by block height and row id descending.",
    params(
        ("lockup_account_id" = String, Path, description = "Lockup account id"),
        VenearLockupActionsQuery
    ),
    tag = "venear",
    operation_id = "listVenearLockupActions",
    responses(
        (status = 200, description = "Actions associated with a lockup", body = VenearLockupActionsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Lockup not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_lockup_actions(
    State(app_state): State<AppState>,
    Path(lockup_account_id): Path<String>,
    Query(query): Query<VenearLockupActionsQuery>,
) -> Result<Json<VenearLockupActionsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_lockup_exists(db.as_ref(), &lockup_account_id).await?;
    let limit = resolve_limit(query.limit);

    let method_name = query
        .method_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string);

    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<HeightIdCursor>)
        .transpose()?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                id,
                lockup_account_id,
                owner_account_id,
                predecessor_account_id,
                receiver_account_id,
                method_name,
                action_source,
                action_index,
                args_json,
                args_base64,
                attached_deposit,
                transaction_hash,
                receipt_id,
                execution_outcome_id,
                success,
                block_height,
                block_timestamp,
                shard_id,
                staking_pool_account_id,
                metadata,
                created_at
            FROM near.near_venear_lockup_actions
            WHERE lockup_account_id = $1
              AND ($2::text IS NULL OR method_name = $2::text)
              AND ($3::boolean IS NULL OR success = $3::boolean)
              AND (
                $4::bigint IS NULL
                OR block_height < $4::bigint
                OR (block_height = $4::bigint AND id < $5::bigint)
              )
            ORDER BY block_height DESC, id DESC
            LIMIT $6
            ",
            vec![
                lockup_account_id.into(),
                method_name.into(),
                query.success.into(),
                cursor.as_ref().map(|value| value.block_height).into(),
                cursor.as_ref().map(|value| value.id).into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(to_lockup_action_dto)
        .collect::<Result<Vec<_>, _>>()?;

    let has_more = data.len() > limit as usize;
    if has_more {
        data.pop();
    }

    let next_cursor = data
        .last()
        .map(|row| {
            encode_cursor(&HeightIdCursor {
                block_height: row.block_height,
                id: row.id,
            })
        })
        .transpose()?;

    Ok(Json(VenearLockupActionsListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

#[allow(clippy::result_large_err)]
fn to_latest_proposal_dto(row: &QueryResult) -> Result<VenearProposalDto, ApiErrorResponse> {
    let id = decode_row_col::<i64>(row, "id", "proposal")?;
    let proposal_id = decode_row_col::<i64>(row, "proposal_id", "proposal")?;
    let latest_event_type = decode_row_col::<String>(row, "event_type", "proposal")?;
    let account_id = decode_row_col::<Option<String>>(row, "account_id", "proposal")?;
    let proposer_id = decode_row_col::<Option<String>>(row, "proposer_id", "proposal")?;
    let reviewer_id = decode_row_col::<Option<String>>(row, "reviewer_id", "proposal")?;
    let voting_start_time_sec =
        decode_row_col::<Option<i64>>(row, "voting_start_time_sec", "proposal")?;
    let snapshot_block_height =
        decode_row_col::<Option<i64>>(row, "snapshot_block_height", "proposal")?;
    let snapshot_root = decode_row_col::<Option<String>>(row, "snapshot_root", "proposal")?;
    let snapshot_total_venear =
        decode_row_col::<Option<String>>(row, "snapshot_total_venear", "proposal")?;
    let metadata = decode_row_col::<Option<serde_json::Value>>(row, "metadata", "proposal")?;
    let payload = decode_row_col::<Option<serde_json::Value>>(row, "payload", "proposal")?;
    let block_height = decode_row_col::<i64>(row, "block_height", "proposal")?;
    let block_timestamp_ns = decode_row_col::<i64>(row, "block_timestamp", "proposal")?;
    let shard_id = decode_row_col::<i32>(row, "shard_id", "proposal")?;
    let receipt_id = decode_row_col::<String>(row, "receipt_id", "proposal")?;
    let execution_outcome_id = decode_row_col::<String>(row, "execution_outcome_id", "proposal")?;
    let predecessor_account_id =
        decode_row_col::<String>(row, "predecessor_account_id", "proposal")?;
    let contract_id = decode_row_col::<String>(row, "contract_id", "proposal")?;
    let log_index = decode_row_col::<i32>(row, "log_index", "proposal")?;
    let data_index = decode_row_col::<i32>(row, "data_index", "proposal")?;
    let created_at = decode_row_col::<NaiveDateTime>(row, "created_at", "proposal")?;

    Ok(VenearProposalDto {
        id,
        proposal_id,
        latest_event_type,
        account_id,
        proposer_id,
        reviewer_id,
        voting_start_time_sec,
        voting_start_time: voting_start_time_sec.map(sec_to_rfc3339).transpose()?,
        snapshot_block_height,
        snapshot_root,
        snapshot_total_venear,
        metadata,
        payload,
        block_height,
        block_timestamp_ns,
        block_timestamp: ns_to_rfc3339(block_timestamp_ns)?,
        shard_id,
        receipt_id,
        execution_outcome_id,
        predecessor_account_id,
        contract_id,
        log_index,
        data_index,
        created_at: naive_to_rfc3339(created_at),
        created_at_epoch: naive_to_epoch(created_at),
    })
}

#[allow(clippy::result_large_err)]
fn to_proposal_event_dto(row: &QueryResult) -> Result<VenearProposalEventDto, ApiErrorResponse> {
    let id = decode_row_col::<i64>(row, "id", "proposal event")?;
    let proposal_id = decode_row_col::<i64>(row, "proposal_id", "proposal event")?;
    let event_type = decode_row_col::<String>(row, "event_type", "proposal event")?;
    let account_id = decode_row_col::<Option<String>>(row, "account_id", "proposal event")?;
    let proposer_id = decode_row_col::<Option<String>>(row, "proposer_id", "proposal event")?;
    let reviewer_id = decode_row_col::<Option<String>>(row, "reviewer_id", "proposal event")?;
    let voting_start_time_sec =
        decode_row_col::<Option<i64>>(row, "voting_start_time_sec", "proposal event")?;
    let snapshot_block_height =
        decode_row_col::<Option<i64>>(row, "snapshot_block_height", "proposal event")?;
    let snapshot_root = decode_row_col::<Option<String>>(row, "snapshot_root", "proposal event")?;
    let snapshot_total_venear =
        decode_row_col::<Option<String>>(row, "snapshot_total_venear", "proposal event")?;
    let metadata = decode_row_col::<Option<serde_json::Value>>(row, "metadata", "proposal event")?;
    let payload = decode_row_col::<Option<serde_json::Value>>(row, "payload", "proposal event")?;
    let block_height = decode_row_col::<i64>(row, "block_height", "proposal event")?;
    let block_timestamp_ns = decode_row_col::<i64>(row, "block_timestamp", "proposal event")?;
    let shard_id = decode_row_col::<i32>(row, "shard_id", "proposal event")?;
    let receipt_id = decode_row_col::<String>(row, "receipt_id", "proposal event")?;
    let execution_outcome_id =
        decode_row_col::<String>(row, "execution_outcome_id", "proposal event")?;
    let predecessor_account_id =
        decode_row_col::<String>(row, "predecessor_account_id", "proposal event")?;
    let contract_id = decode_row_col::<String>(row, "contract_id", "proposal event")?;
    let log_index = decode_row_col::<i32>(row, "log_index", "proposal event")?;
    let data_index = decode_row_col::<i32>(row, "data_index", "proposal event")?;
    let created_at = decode_row_col::<NaiveDateTime>(row, "created_at", "proposal event")?;

    Ok(VenearProposalEventDto {
        id,
        proposal_id,
        event_type,
        account_id,
        proposer_id,
        reviewer_id,
        voting_start_time_sec,
        voting_start_time: voting_start_time_sec.map(sec_to_rfc3339).transpose()?,
        snapshot_block_height,
        snapshot_root,
        snapshot_total_venear,
        metadata,
        payload,
        block_height,
        block_timestamp_ns,
        block_timestamp: ns_to_rfc3339(block_timestamp_ns)?,
        shard_id,
        receipt_id,
        execution_outcome_id,
        predecessor_account_id,
        contract_id,
        log_index,
        data_index,
        created_at: naive_to_rfc3339(created_at),
        created_at_epoch: naive_to_epoch(created_at),
    })
}

#[allow(clippy::result_large_err)]
fn to_vote_dto(row: &QueryResult) -> Result<VenearVoteDto, ApiErrorResponse> {
    let id = decode_row_col::<i64>(row, "id", "vote")?;
    let proposal_id = decode_row_col::<i64>(row, "proposal_id", "vote")?;
    let account_id = decode_row_col::<String>(row, "account_id", "vote")?;
    let vote = decode_row_col::<i32>(row, "vote", "vote")?;
    let account_balance = decode_row_col::<String>(row, "account_balance", "vote")?;
    let event_type = decode_row_col::<String>(row, "event_type", "vote")?;
    let payload = decode_row_col::<Option<serde_json::Value>>(row, "payload", "vote")?;
    let block_height = decode_row_col::<i64>(row, "block_height", "vote")?;
    let block_timestamp_ns = decode_row_col::<i64>(row, "block_timestamp", "vote")?;
    let shard_id = decode_row_col::<i32>(row, "shard_id", "vote")?;
    let receipt_id = decode_row_col::<String>(row, "receipt_id", "vote")?;
    let execution_outcome_id = decode_row_col::<String>(row, "execution_outcome_id", "vote")?;
    let predecessor_account_id = decode_row_col::<String>(row, "predecessor_account_id", "vote")?;
    let contract_id = decode_row_col::<String>(row, "contract_id", "vote")?;
    let log_index = decode_row_col::<i32>(row, "log_index", "vote")?;
    let data_index = decode_row_col::<i32>(row, "data_index", "vote")?;
    let created_at = decode_row_col::<NaiveDateTime>(row, "created_at", "vote")?;

    Ok(VenearVoteDto {
        id,
        proposal_id,
        account_id,
        vote,
        account_balance,
        event_type,
        payload,
        block_height,
        block_timestamp_ns,
        block_timestamp: ns_to_rfc3339(block_timestamp_ns)?,
        shard_id,
        receipt_id,
        execution_outcome_id,
        predecessor_account_id,
        contract_id,
        log_index,
        data_index,
        created_at: naive_to_rfc3339(created_at),
        created_at_epoch: naive_to_epoch(created_at),
    })
}

#[allow(clippy::result_large_err)]
fn to_account_balance_dto(row: &QueryResult) -> Result<VenearAccountBalanceDto, ApiErrorResponse> {
    let id = decode_row_col::<i64>(row, "id", "account balance")?;
    let account_id = decode_row_col::<String>(row, "account_id", "account balance")?;
    let event_type = decode_row_col::<String>(row, "event_type", "account balance")?;
    let delta_amount = decode_row_col::<Option<String>>(row, "delta_amount", "account balance")?;
    let total_balance = decode_row_col::<String>(row, "total_balance", "account balance")?;
    let locked_near_balance =
        decode_row_col::<Option<String>>(row, "locked_near_balance", "account balance")?;
    let lockup_update_nonce =
        decode_row_col::<Option<i64>>(row, "lockup_update_nonce", "account balance")?;
    let lockup_version = decode_row_col::<Option<i64>>(row, "lockup_version", "account balance")?;
    let block_height = decode_row_col::<i64>(row, "block_height", "account balance")?;
    let block_timestamp_ns = decode_row_col::<i64>(row, "block_timestamp", "account balance")?;
    let shard_id = decode_row_col::<i32>(row, "shard_id", "account balance")?;
    let receipt_id = decode_row_col::<String>(row, "receipt_id", "account balance")?;
    let execution_outcome_id =
        decode_row_col::<String>(row, "execution_outcome_id", "account balance")?;
    let predecessor_account_id =
        decode_row_col::<String>(row, "predecessor_account_id", "account balance")?;
    let contract_id = decode_row_col::<String>(row, "contract_id", "account balance")?;
    let log_index = decode_row_col::<i32>(row, "log_index", "account balance")?;
    let data_index = decode_row_col::<i32>(row, "data_index", "account balance")?;
    let payload = decode_row_col::<Option<serde_json::Value>>(row, "payload", "account balance")?;
    let created_at = decode_row_col::<NaiveDateTime>(row, "created_at", "account balance")?;

    Ok(VenearAccountBalanceDto {
        id,
        account_id,
        event_type,
        delta_amount,
        total_balance,
        locked_near_balance,
        lockup_update_nonce,
        lockup_version,
        block_height,
        block_timestamp_ns,
        block_timestamp: ns_to_rfc3339(block_timestamp_ns)?,
        shard_id,
        receipt_id,
        execution_outcome_id,
        predecessor_account_id,
        contract_id,
        log_index,
        data_index,
        payload,
        created_at: naive_to_rfc3339(created_at),
        created_at_epoch: naive_to_epoch(created_at),
    })
}

#[allow(clippy::result_large_err)]
fn to_lockup_dto(row: &QueryResult) -> Result<VenearLockupDto, ApiErrorResponse> {
    let lockup_account_id = decode_row_col::<String>(row, "lockup_account_id", "lockup")?;
    let owner_account_id = decode_row_col::<String>(row, "owner_account_id", "lockup")?;
    let lockup_version = decode_row_col::<Option<i64>>(row, "lockup_version", "lockup")?;
    let discovery_source = decode_row_col::<String>(row, "discovery_source", "lockup")?;
    let discovered_at_height = decode_row_col::<i64>(row, "discovered_at_height", "lockup")?;
    let discovered_at_timestamp_ns =
        decode_row_col::<i64>(row, "discovered_at_timestamp", "lockup")?;
    let last_seen_height = decode_row_col::<i64>(row, "last_seen_height", "lockup")?;
    let last_seen_timestamp_ns = decode_row_col::<i64>(row, "last_seen_timestamp", "lockup")?;
    let is_active = decode_row_col::<bool>(row, "is_active", "lockup")?;
    let deleted_at_height = decode_row_col::<Option<i64>>(row, "deleted_at_height", "lockup")?;
    let deleted_at_timestamp_ns =
        decode_row_col::<Option<i64>>(row, "deleted_at_timestamp", "lockup")?;
    let metadata = decode_row_col::<Option<serde_json::Value>>(row, "metadata", "lockup")?;

    Ok(VenearLockupDto {
        lockup_account_id,
        owner_account_id,
        lockup_version,
        discovery_source,
        discovered_at_height,
        discovered_at_timestamp_ns,
        discovered_at_timestamp: ns_to_rfc3339(discovered_at_timestamp_ns)?,
        last_seen_height,
        last_seen_timestamp_ns,
        last_seen_timestamp: ns_to_rfc3339(last_seen_timestamp_ns)?,
        is_active,
        deleted_at_height,
        deleted_at_timestamp_ns,
        deleted_at_timestamp: deleted_at_timestamp_ns.map(ns_to_rfc3339).transpose()?,
        metadata,
    })
}

#[allow(clippy::result_large_err)]
fn to_lockup_action_dto(row: &QueryResult) -> Result<VenearLockupActionDto, ApiErrorResponse> {
    let id = decode_row_col::<i64>(row, "id", "lockup action")?;
    let lockup_account_id = decode_row_col::<String>(row, "lockup_account_id", "lockup action")?;
    let owner_account_id =
        decode_row_col::<Option<String>>(row, "owner_account_id", "lockup action")?;
    let predecessor_account_id =
        decode_row_col::<String>(row, "predecessor_account_id", "lockup action")?;
    let receiver_account_id =
        decode_row_col::<String>(row, "receiver_account_id", "lockup action")?;
    let method_name = decode_row_col::<String>(row, "method_name", "lockup action")?;
    let action_source = decode_row_col::<String>(row, "action_source", "lockup action")?;
    let action_index = decode_row_col::<i32>(row, "action_index", "lockup action")?;
    let args_json = decode_row_col::<Option<serde_json::Value>>(row, "args_json", "lockup action")?;
    let args_base64 = decode_row_col::<String>(row, "args_base64", "lockup action")?;
    let attached_deposit = decode_row_col::<String>(row, "attached_deposit", "lockup action")?;
    let transaction_hash =
        decode_row_col::<Option<String>>(row, "transaction_hash", "lockup action")?;
    let receipt_id = decode_row_col::<String>(row, "receipt_id", "lockup action")?;
    let execution_outcome_id =
        decode_row_col::<String>(row, "execution_outcome_id", "lockup action")?;
    let success = decode_row_col::<bool>(row, "success", "lockup action")?;
    let block_height = decode_row_col::<i64>(row, "block_height", "lockup action")?;
    let block_timestamp_ns = decode_row_col::<i64>(row, "block_timestamp", "lockup action")?;
    let shard_id = decode_row_col::<i32>(row, "shard_id", "lockup action")?;
    let staking_pool_account_id =
        decode_row_col::<Option<String>>(row, "staking_pool_account_id", "lockup action")?;
    let metadata = decode_row_col::<Option<serde_json::Value>>(row, "metadata", "lockup action")?;
    let created_at = decode_row_col::<NaiveDateTime>(row, "created_at", "lockup action")?;

    Ok(VenearLockupActionDto {
        id,
        lockup_account_id,
        owner_account_id,
        predecessor_account_id,
        receiver_account_id,
        method_name,
        action_source,
        action_index,
        args_json,
        args_base64,
        attached_deposit,
        transaction_hash,
        receipt_id,
        execution_outcome_id,
        success,
        block_height,
        block_timestamp_ns,
        block_timestamp: ns_to_rfc3339(block_timestamp_ns)?,
        shard_id,
        staking_pool_account_id,
        metadata,
        created_at: naive_to_rfc3339(created_at),
        created_at_epoch: naive_to_epoch(created_at),
    })
}

#[allow(clippy::result_large_err)]
fn to_delegation_event_dto(
    row: &QueryResult,
) -> Result<VenearDelegationEventDto, ApiErrorResponse> {
    let id = decode_row_col::<i64>(row, "id", "delegation event")?;
    let delegator_account_id =
        decode_row_col::<String>(row, "delegator_account_id", "delegation event")?;
    let delegatee_account_id =
        decode_row_col::<Option<String>>(row, "delegatee_account_id", "delegation event")?;
    let event_type = decode_row_col::<String>(row, "event_type", "delegation event")?;
    let action_source = decode_row_col::<String>(row, "action_source", "delegation event")?;
    let action_index = decode_row_col::<i32>(row, "action_index", "delegation event")?;
    let args_json =
        decode_row_col::<Option<serde_json::Value>>(row, "args_json", "delegation event")?;
    let args_base64 = decode_row_col::<String>(row, "args_base64", "delegation event")?;
    let attached_deposit = decode_row_col::<String>(row, "attached_deposit", "delegation event")?;
    let transaction_hash =
        decode_row_col::<Option<String>>(row, "transaction_hash", "delegation event")?;
    let receipt_id = decode_row_col::<String>(row, "receipt_id", "delegation event")?;
    let execution_outcome_id =
        decode_row_col::<String>(row, "execution_outcome_id", "delegation event")?;
    let success = decode_row_col::<bool>(row, "success", "delegation event")?;
    let block_height = decode_row_col::<i64>(row, "block_height", "delegation event")?;
    let block_timestamp_ns = decode_row_col::<i64>(row, "block_timestamp", "delegation event")?;
    let shard_id = decode_row_col::<i32>(row, "shard_id", "delegation event")?;
    let contract_id = decode_row_col::<String>(row, "contract_id", "delegation event")?;
    let metadata =
        decode_row_col::<Option<serde_json::Value>>(row, "metadata", "delegation event")?;
    let created_at = decode_row_col::<NaiveDateTime>(row, "created_at", "delegation event")?;

    Ok(VenearDelegationEventDto {
        id,
        delegator_account_id,
        delegatee_account_id,
        event_type,
        action_source,
        action_index,
        args_json,
        args_base64,
        attached_deposit,
        transaction_hash,
        receipt_id,
        execution_outcome_id,
        success,
        block_height,
        block_timestamp_ns,
        block_timestamp: ns_to_rfc3339(block_timestamp_ns)?,
        shard_id,
        contract_id,
        metadata,
        created_at: naive_to_rfc3339(created_at),
        created_at_epoch: naive_to_epoch(created_at),
    })
}

fn resolve_limit(limit: Option<u64>) -> u64 {
    limit.unwrap_or(20).clamp(1, 100)
}

#[allow(clippy::result_large_err)]
fn require_db(app_state: &AppState) -> Result<Arc<DatabaseConnection>, ApiErrorResponse> {
    app_state
        .db
        .clone()
        .ok_or_else(|| ApiErrorResponse::service_unavailable("Database connection unavailable"))
}

async fn ensure_proposal_exists(
    db: &DatabaseConnection,
    proposal_id: i64,
) -> Result<(), ApiErrorResponse> {
    let count = scalar_i64_with_values(
        db,
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_proposal_events WHERE proposal_id = $1",
        vec![proposal_id.into()],
    )
    .await?;

    if count > 0 {
        Ok(())
    } else {
        Err(ApiErrorResponse::not_found("Proposal not found"))
    }
}

async fn ensure_account_balance_exists(
    db: &DatabaseConnection,
    account_id: &str,
) -> Result<(), ApiErrorResponse> {
    let count = scalar_i64_with_values(
        db,
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_account_balances WHERE account_id = $1",
        vec![account_id.into()],
    )
    .await?;

    if count > 0 {
        Ok(())
    } else {
        Err(ApiErrorResponse::not_found("Account not found"))
    }
}

async fn ensure_delegation_account_exists(
    db: &DatabaseConnection,
    account_id: &str,
) -> Result<(), ApiErrorResponse> {
    let count = scalar_i64_with_values(
        db,
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_venear_delegation_events
         WHERE delegator_account_id = $1 OR delegatee_account_id = $1",
        vec![account_id.into()],
    )
    .await?;

    if count > 0 {
        Ok(())
    } else {
        Err(ApiErrorResponse::not_found("Account not found"))
    }
}

async fn ensure_lockup_exists(
    db: &DatabaseConnection,
    lockup_account_id: &str,
) -> Result<(), ApiErrorResponse> {
    let count = scalar_i64_with_values(
        db,
        "SELECT COUNT(*)::bigint AS value FROM near.near_venear_lockups WHERE lockup_account_id = $1",
        vec![lockup_account_id.into()],
    )
    .await?;

    if count > 0 {
        Ok(())
    } else {
        Err(ApiErrorResponse::not_found("Lockup not found"))
    }
}

async fn scalar_i64_with_values(
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

fn naive_to_rfc3339(value: NaiveDateTime) -> String {
    value.and_utc().to_rfc3339()
}

fn naive_to_epoch(value: NaiveDateTime) -> i64 {
    value.and_utc().timestamp()
}

#[allow(clippy::result_large_err)]
fn sec_to_rfc3339(epoch_seconds: i64) -> Result<String, ApiErrorResponse> {
    DateTime::<Utc>::from_timestamp(epoch_seconds, 0)
        .map(|value| value.to_rfc3339())
        .ok_or_else(|| {
            ApiErrorResponse::internal_server_error("Invalid epoch seconds in stored data")
        })
}

#[allow(clippy::result_large_err)]
fn ns_to_rfc3339(epoch_nanoseconds: i64) -> Result<String, ApiErrorResponse> {
    let seconds = epoch_nanoseconds.div_euclid(1_000_000_000);
    let nanos = epoch_nanoseconds.rem_euclid(1_000_000_000) as u32;

    DateTime::<Utc>::from_timestamp(seconds, nanos)
        .map(|value| value.to_rfc3339())
        .ok_or_else(|| {
            ApiErrorResponse::internal_server_error("Invalid epoch nanoseconds in stored data")
        })
}

#[allow(clippy::result_large_err)]
fn decode_row_col<T: sea_orm::TryGetable>(
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

#[allow(clippy::result_large_err)]
fn encode_cursor<T: Serialize>(payload: &T) -> Result<String, ApiErrorResponse> {
    let bytes = serde_json::to_vec(payload).map_err(|err| {
        ApiErrorResponse::internal_server_error("Failed to encode pagination cursor")
            .with_detail(err.to_string())
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[allow(clippy::result_large_err)]
fn decode_cursor<T: DeserializeOwned>(cursor: &str) -> Result<T, ApiErrorResponse> {
    let bytes = URL_SAFE_NO_PAD.decode(cursor).map_err(|err| {
        ApiErrorResponse::bad_request("Invalid cursor value").with_detail(err.to_string())
    })?;

    serde_json::from_slice::<T>(&bytes).map_err(|err| {
        ApiErrorResponse::bad_request("Invalid cursor payload").with_detail(err.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
    struct CursorFixture {
        id: i64,
    }

    #[test]
    fn cursor_roundtrip_works() {
        let encoded = encode_cursor(&CursorFixture { id: 9 }).expect("encode cursor");
        let decoded: CursorFixture = decode_cursor(&encoded).expect("decode cursor");
        assert_eq!(decoded, CursorFixture { id: 9 });
    }

    #[test]
    fn limit_is_bounded() {
        assert_eq!(resolve_limit(None), 20);
        assert_eq!(resolve_limit(Some(0)), 1);
        assert_eq!(resolve_limit(Some(2)), 2);
        assert_eq!(resolve_limit(Some(10_000)), 100);
    }

    #[test]
    fn timestamp_conversions_work() {
        assert_eq!(
            ns_to_rfc3339(1_760_000_000_000_000_000).expect("ns to rfc3339"),
            "2025-10-09T08:53:20+00:00"
        );
        assert_eq!(
            sec_to_rfc3339(1_760_000_000).expect("sec to rfc3339"),
            "2025-10-09T08:53:20+00:00"
        );
    }
}
