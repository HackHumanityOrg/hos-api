//! Ergonomic veNEAR governance read API handlers.

use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Path, Query, State},
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chrono::{DateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, Value,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use serde_json::Value as JsonValue;
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
struct DelegateCursor {
    account_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ConfigCursor {
    block_height: i64,
    id: i64,
    field_order: i32,
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/config/current",
    summary = "Get current veNEAR config",
    description = "Returns the latest indexed config value per `(contract_id, field)` for guardians and reviewer ids.",
    tag = "venear",
    operation_id = "getVenearConfigCurrent",
    responses(
        (status = 200, description = "Latest veNEAR config rows", body = VenearConfigCurrentResponse),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_config_current(
    State(app_state): State<AppState>,
) -> Result<Json<VenearConfigCurrentResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT DISTINCT ON (contract_id, config_field)
                id,
                contract_id,
                method_name,
                predecessor_account_id,
                transaction_hash,
                receipt_id,
                execution_outcome_id,
                block_height,
                block_timestamp,
                config_field,
                config_value,
                config_source,
                field_order
            FROM near.hos_config_history
            ORDER BY contract_id ASC, config_field ASC, block_height DESC, id DESC
            ",
            vec![],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let data = rows
        .iter()
        .map(to_config_entry_dto)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Json(VenearConfigCurrentResponse { data }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/config/history",
    summary = "List veNEAR config history",
    description = "Returns normalized config updates from `new`, `set_guardians`, and `set_reviewer_ids` method calls, ordered by block height and id descending.",
    params(VenearConfigHistoryQuery),
    tag = "venear",
    operation_id = "listVenearConfigHistory",
    responses(
        (status = 200, description = "veNEAR config history rows", body = VenearConfigHistoryListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_config_history(
    State(app_state): State<AppState>,
    Query(query): Query<VenearConfigHistoryQuery>,
) -> Result<Json<VenearConfigHistoryListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.limit);

    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<ConfigCursor>)
        .transpose()?;

    let contract_id = normalized_contract_id_filter(query.contract_id.as_deref());
    let field = parse_config_field_filter(query.field.as_deref())?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                id,
                contract_id,
                method_name,
                predecessor_account_id,
                transaction_hash,
                receipt_id,
                execution_outcome_id,
                block_height,
                block_timestamp,
                config_field,
                config_value,
                config_source,
                field_order
            FROM near.hos_config_history
            WHERE (
                $1::bigint IS NULL
                OR block_height < $1::bigint
                OR (block_height = $1::bigint AND id < $2::bigint)
                OR (
                    block_height = $1::bigint
                    AND id = $2::bigint
                    AND field_order > $3::integer
                )
            )
              AND ($4::text IS NULL OR contract_id = $4::text)
              AND ($5::text IS NULL OR config_field = $5::text)
            ORDER BY block_height DESC, id DESC, field_order ASC
            LIMIT $6
            ",
            vec![
                cursor.as_ref().map(|value| value.block_height).into(),
                cursor.as_ref().map(|value| value.id).into(),
                cursor.as_ref().map(|value| value.field_order).into(),
                contract_id.into(),
                field.into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(to_config_entry_with_cursor)
        .collect::<Result<Vec<_>, _>>()?;

    let has_more = data.len() > limit as usize;
    if has_more {
        data.pop();
    }

    let next_cursor = data
        .last()
        .map(|row| encode_cursor(&row.cursor))
        .transpose()?;

    Ok(Json(VenearConfigHistoryListResponse {
        data: data.into_iter().map(|row| row.dto).collect(),
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/proposals",
    summary = "List veNEAR proposals",
    description = "Returns one latest-state projection per proposal, ordered by proposal id descending.",
    params(VenearProposalsQuery),
    tag = "venear",
    operation_id = "listVenearProposals",
    responses(
        (status = 200, description = "Latest proposal states", body = VenearProposalsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_proposals(
    State(app_state): State<AppState>,
    Query(query): Query<VenearProposalsQuery>,
) -> Result<Json<VenearProposalsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.limit);

    let proposal_cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<ProposalCursor>)
        .transpose()?
        .map(|cursor| cursor.proposal_id);

    let latest_event_type = normalized_event_type_filter(query.latest_event_type.as_deref());
    let proposer_id = normalized_text_filter(query.proposer_id.as_deref());

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            WITH latest AS (
                SELECT
                    0::bigint AS id,
                    p.proposal_id::bigint AS proposal_id,
                    CASE
                        WHEN p.is_rejected THEN 'proposal_reject'
                        WHEN p.is_approved AND p.voting_start_at IS NOT NULL THEN 'proposal_approve_finalized'
                        WHEN p.is_approved THEN 'proposal_approve'
                        ELSE 'create_proposal'
                    END AS event_type,
                    NULL::text AS account_id,
                    p.creator_id AS proposer_id,
                    p.approver_id AS reviewer_id,
                    CASE
                        WHEN p.voting_start_at IS NULL THEN NULL
                        ELSE EXTRACT(EPOCH FROM p.voting_start_at)::bigint
                    END AS voting_start_time_sec,
                    NULL::bigint AS snapshot_block_height,
                    NULL::text AS snapshot_root,
                    p.total_venear_at_approval::text AS snapshot_total_venear,
                    p.proposal_metadata AS metadata,
                    p.proposal_payload AS payload,
                    p.block_height,
                    p.block_timestamp,
                    p.receipt_id,
                    p.execution_outcome_id,
                    p.voting_options,
                    p.num_distinct_voters,
                    p.for_voting_power,
                    p.against_voting_power,
                    p.abstain_voting_power,
                    p.created_at
                FROM near.hos_proposals p
                WHERE ($1::bigint IS NULL OR p.proposal_id < $1::bigint)
            )
            SELECT
                l.id,
                l.proposal_id,
                l.event_type,
                l.account_id,
                l.proposer_id,
                l.reviewer_id,
                l.voting_start_time_sec,
                l.snapshot_block_height,
                l.snapshot_root,
                l.snapshot_total_venear,
                l.metadata,
                l.payload,
                l.block_height,
                l.block_timestamp,
                l.receipt_id,
                l.execution_outcome_id,
                l.proposer_id AS create_proposer_id,
                l.metadata AS create_metadata,
                CASE
                    WHEN l.created_at IS NULL THEN NULL
                    ELSE (EXTRACT(EPOCH FROM l.created_at) * 1000000000)::bigint
                END AS created_block_timestamp,
                COALESCE(l.num_distinct_voters, 0)::bigint AS effective_votes_count,
                (
                    COALESCE(l.for_voting_power, 0)
                    + COALESCE(l.against_voting_power, 0)
                    + COALESCE(l.abstain_voting_power, 0)
                )::text AS effective_votes_voting_power
            FROM latest l
            WHERE ($2::text IS NULL OR l.event_type = $2::text)
              AND ($3::text IS NULL OR l.proposer_id = $3::text)
            ORDER BY l.proposal_id DESC
            LIMIT $4
            ",
            vec![
                proposal_cursor.into(),
                latest_event_type.into(),
                proposer_id.into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(to_proposal_summary_dto)
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
    path = "/api/v1/venear/proposals/{proposal_id}",
    summary = "Get one veNEAR proposal",
    description = "Returns the latest proposal view with vote aggregates and latest event metadata.",
    params(("proposal_id" = i64, Path, description = "Proposal id")),
    tag = "venear",
    operation_id = "getVenearProposal",
    responses(
        (status = 200, description = "Proposal detail", body = VenearProposalDto),
        (status = 404, description = "Proposal not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn get_proposal(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<i64>,
) -> Result<Json<VenearProposalDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;

    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                0::bigint AS id,
                p.proposal_id::bigint AS proposal_id,
                CASE
                    WHEN p.is_rejected THEN 'proposal_reject'
                    WHEN p.is_approved AND p.voting_start_at IS NOT NULL THEN 'proposal_approve_finalized'
                    WHEN p.is_approved THEN 'proposal_approve'
                    ELSE 'create_proposal'
                END AS event_type,
                NULL::text AS account_id,
                p.creator_id AS proposer_id,
                p.approver_id AS reviewer_id,
                CASE
                    WHEN p.voting_start_at IS NULL THEN NULL
                    ELSE EXTRACT(EPOCH FROM p.voting_start_at)::bigint
                END AS voting_start_time_sec,
                NULL::bigint AS snapshot_block_height,
                NULL::text AS snapshot_root,
                p.total_venear_at_approval::text AS snapshot_total_venear,
                p.proposal_metadata AS metadata,
                p.proposal_payload AS payload,
                p.block_height,
                p.block_timestamp,
                p.transaction_hash,
                p.receipt_id,
                p.execution_outcome_id,
                p.creator_id AS create_proposer_id,
                p.proposal_metadata AS create_metadata,
                CASE
                    WHEN p.created_at IS NULL THEN NULL
                    ELSE (EXTRACT(EPOCH FROM p.created_at) * 1000000000)::bigint
                END AS created_block_timestamp,
                COALESCE(p.num_distinct_voters, 0)::bigint AS effective_votes_count,
                (
                    COALESCE(p.for_voting_power, 0)
                    + COALESCE(p.against_voting_power, 0)
                    + COALESCE(p.abstain_voting_power, 0)
                )::text AS effective_votes_voting_power
            FROM near.hos_proposals p
            WHERE p.proposal_id = $1
            LIMIT 1
            ",
            vec![proposal_id.into()],
        ))
        .await
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::not_found("Proposal not found"))?;

    let summary = to_proposal_summary_dto(&row)?;
    let vote_aggregates =
        fetch_vote_aggregates(db.as_ref(), proposal_id, &summary.voting_options).await?;

    Ok(Json(VenearProposalDto {
        proposal_id: summary.proposal_id,
        title: summary.title,
        description: summary.description,
        link: summary.link,
        voting_options: summary.voting_options,
        status: summary.status,
        proposer_id: summary.proposer_id,
        reviewer_id: summary.reviewer_id,
        created_at: summary.created_at,
        voting_start_at: summary.voting_start_at,
        voting_end_at: summary.voting_end_at,
        snapshot_block_height: summary.snapshot_block_height,
        snapshot_root: summary.snapshot_root,
        snapshot_total_venear: summary.snapshot_total_venear,
        effective_votes_count: summary.effective_votes_count,
        effective_votes_voting_power: summary.effective_votes_voting_power,
        vote_aggregates,
        latest_event: VenearProposalLatestEventDto {
            event_type: summary.latest_event_type,
            block_height: summary.latest_block_height,
            block_timestamp: summary.latest_block_timestamp,
            transaction_hash: decode_row_col::<Option<String>>(
                &row,
                "transaction_hash",
                "proposal",
            )?,
            receipt_id: decode_row_col::<String>(&row, "receipt_id", "proposal")?,
            execution_outcome_id: decode_row_col::<String>(
                &row,
                "execution_outcome_id",
                "proposal",
            )?,
        },
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/proposals/{proposal_id}/events",
    summary = "List proposal events",
    description = "Returns proposal events ordered by block height and row id descending.",
    params(
        ("proposal_id" = i64, Path, description = "Proposal id"),
        VenearProposalEventsQuery
    ),
    tag = "venear",
    operation_id = "listVenearProposalEvents",
    responses(
        (status = 200, description = "Proposal events", body = VenearProposalEventsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Proposal not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_proposal_events(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<i64>,
    Query(query): Query<VenearProposalEventsQuery>,
) -> Result<Json<VenearProposalEventsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_proposal_exists(db.as_ref(), proposal_id).await?;
    let limit = resolve_limit(query.limit);

    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<HeightIdCursor>)
        .transpose()?;

    let event_type = normalized_event_type_filter(query.event_type.as_deref());
    let actor_account_id = normalized_text_filter(query.actor_account_id.as_deref());

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            WITH base_proposal AS (
                SELECT *
                FROM near.hos_proposals
                WHERE proposal_id = $1
            ),
            events AS (
                SELECT
                    bp.proposal_id::bigint AS proposal_id,
                    'create_proposal'::text AS event_type,
                    bp.creator_id AS account_id,
                    bp.creator_id AS proposer_id,
                    bp.approver_id AS reviewer_id,
                    CASE
                        WHEN bp.voting_start_at IS NULL THEN NULL
                        ELSE EXTRACT(EPOCH FROM bp.voting_start_at)::bigint
                    END AS voting_start_time_sec,
                    NULL::bigint AS snapshot_block_height,
                    NULL::text AS snapshot_root,
                    bp.total_venear_at_approval::text AS snapshot_total_venear,
                    bp.proposal_metadata AS metadata,
                    bp.proposal_payload AS payload,
                    bp.block_height,
                    bp.block_timestamp,
                    bp.transaction_hash,
                    bp.receipt_id,
                    bp.execution_outcome_id,
                    0::integer AS event_order
                FROM base_proposal bp

                UNION ALL

                SELECT
                    bp.proposal_id::bigint AS proposal_id,
                    'proposal_approve'::text AS event_type,
                    COALESCE(ap.proposal_approver_id, bp.approver_id) AS account_id,
                    bp.creator_id AS proposer_id,
                    COALESCE(ap.proposal_approver_id, bp.approver_id) AS reviewer_id,
                    CASE
                        WHEN bp.voting_start_at IS NULL THEN NULL
                        ELSE EXTRACT(EPOCH FROM bp.voting_start_at)::bigint
                    END AS voting_start_time_sec,
                    NULL::bigint AS snapshot_block_height,
                    NULL::text AS snapshot_root,
                    bp.total_venear_at_approval::text AS snapshot_total_venear,
                    bp.proposal_metadata AS metadata,
                    bp.proposal_payload AS payload,
                    COALESCE(ap.block_height, bp.block_height) AS block_height,
                    COALESCE(
                        ap.block_timestamp,
                        (EXTRACT(EPOCH FROM bp.approved_at) * 1000000000)::bigint,
                        bp.block_timestamp
                    ) AS block_timestamp,
                    COALESCE(ap.transaction_hash, bp.transaction_hash) AS transaction_hash,
                    COALESCE(ap.receipt_id, bp.receipt_id) AS receipt_id,
                    COALESCE(ap.execution_outcome_id, bp.execution_outcome_id) AS execution_outcome_id,
                    1::integer AS event_order
                FROM base_proposal bp
                LEFT JOIN LATERAL (
                    SELECT
                        ap.proposal_approver_id,
                        ap.block_height,
                        ap.block_timestamp,
                        ap.transaction_hash,
                        ap.receipt_id,
                        ap.execution_outcome_id
                    FROM near.hos_approved_proposals ap
                    WHERE ap.proposal_id = bp.proposal_id
                    ORDER BY ap.block_timestamp DESC, ap.receipt_id DESC
                    LIMIT 1
                ) ap ON TRUE
                WHERE bp.is_approved = true

                UNION ALL

                SELECT
                    bp.proposal_id::bigint AS proposal_id,
                    'proposal_reject'::text AS event_type,
                    COALESCE(rp.rejecter_id, bp.rejecter_id) AS account_id,
                    bp.creator_id AS proposer_id,
                    COALESCE(rp.rejecter_id, bp.rejecter_id) AS reviewer_id,
                    CASE
                        WHEN bp.voting_start_at IS NULL THEN NULL
                        ELSE EXTRACT(EPOCH FROM bp.voting_start_at)::bigint
                    END AS voting_start_time_sec,
                    NULL::bigint AS snapshot_block_height,
                    NULL::text AS snapshot_root,
                    bp.total_venear_at_approval::text AS snapshot_total_venear,
                    bp.proposal_metadata AS metadata,
                    bp.proposal_payload AS payload,
                    COALESCE(rp.block_height, bp.block_height) AS block_height,
                    COALESCE(
                        rp.block_timestamp,
                        (EXTRACT(EPOCH FROM bp.rejected_at) * 1000000000)::bigint,
                        bp.block_timestamp
                    ) AS block_timestamp,
                    COALESCE(rp.transaction_hash, bp.transaction_hash) AS transaction_hash,
                    COALESCE(rp.receipt_id, bp.receipt_id) AS receipt_id,
                    COALESCE(rp.execution_outcome_id, bp.execution_outcome_id) AS execution_outcome_id,
                    2::integer AS event_order
                FROM base_proposal bp
                LEFT JOIN LATERAL (
                    SELECT
                        ra.signer_account_id AS rejecter_id,
                        ra.block_height,
                        ra.block_timestamp,
                        ra.transaction_hash,
                        ra.receipt_id,
                        ra.execution_outcome_id
                    FROM near.near_receipt_actions ra
                    INNER JOIN near.near_execution_outcomes eo
                        ON eo.receipt_id = ra.receipt_id
                       AND near.is_hos_contract(eo.executor_account_id) = TRUE
                    WHERE ra.method_name = 'reject_proposal'
                      AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                      AND COALESCE(ra.args_json ->> 'proposal_id', '') ~ '^[0-9]+$'
                      AND (ra.args_json ->> 'proposal_id')::NUMERIC = bp.proposal_id
                      AND eo.status IN ('SuccessReceiptId', 'SuccessValue')
                    ORDER BY ra.block_timestamp DESC, ra.receipt_id DESC
                    LIMIT 1
                ) rp ON TRUE
                WHERE bp.is_rejected = true
            ),
            ranked AS (
                SELECT
                    ROW_NUMBER() OVER (ORDER BY block_timestamp ASC, event_order ASC)::bigint AS id,
                    *
                FROM events
            )
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
                transaction_hash,
                receipt_id,
                execution_outcome_id
            FROM ranked
            WHERE proposal_id = $1
              AND (
                $2::bigint IS NULL
                OR block_height < $2::bigint
                OR (block_height = $2::bigint AND id < $3::bigint)
              )
              AND ($4::text IS NULL OR event_type = $4::text)
              AND (
                $5::text IS NULL
                OR COALESCE(account_id, proposer_id, reviewer_id) = $5::text
              )
            ORDER BY block_height DESC, id DESC
            LIMIT $6
            ",
            vec![
                proposal_id.into(),
                cursor.as_ref().map(|value| value.block_height).into(),
                cursor.as_ref().map(|value| value.id).into(),
                event_type.into(),
                actor_account_id.into(),
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
    summary = "List effective proposal votes",
    description = "Returns one latest effective vote per voter for the proposal, ordered by block height and row id descending.",
    params(
        ("proposal_id" = i64, Path, description = "Proposal id"),
        VenearProposalVotesQuery
    ),
    tag = "venear",
    operation_id = "listVenearProposalVotes",
    responses(
        (status = 200, description = "Effective proposal votes", body = VenearVotesListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 404, description = "Proposal not found", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_proposal_votes(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<i64>,
    Query(query): Query<VenearProposalVotesQuery>,
) -> Result<Json<VenearVotesListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    ensure_proposal_exists(db.as_ref(), proposal_id).await?;
    let limit = resolve_limit(query.limit);

    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<HeightIdCursor>)
        .transpose()?;

    let voter_account_id = normalized_text_filter(query.voter_account_id.as_deref());
    let vote_option = query.vote_option;

    let voting_options = fetch_proposal_voting_options(db.as_ref(), proposal_id).await?;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            WITH latest_votes AS (
                SELECT
                    ROW_NUMBER() OVER (ORDER BY block_timestamp DESC, receipt_id DESC)::bigint AS id,
                    voter_id AS account_id,
                    COALESCE(vote_option, 0)::integer AS vote,
                    COALESCE(voting_power, 0)::text AS account_balance,
                    COALESCE(event_type, 'add_vote') AS event_type,
                    block_height,
                    block_timestamp,
                    transaction_hash,
                    receipt_id,
                    execution_outcome_id
                FROM near.hos_proposal_voting_history
                WHERE proposal_id = $1
            )
            SELECT
                id,
                account_id,
                vote,
                account_balance,
                event_type,
                block_height,
                block_timestamp,
                transaction_hash,
                receipt_id,
                execution_outcome_id
            FROM latest_votes
            WHERE event_type = 'add_vote'
              AND (
                $2::bigint IS NULL
                OR block_height < $2::bigint
                OR (block_height = $2::bigint AND id < $3::bigint)
              )
              AND ($4::text IS NULL OR account_id = $4::text)
              AND ($5::integer IS NULL OR vote = $5::integer)
            ORDER BY block_height DESC, id DESC
            LIMIT $6
            ",
            vec![
                proposal_id.into(),
                cursor.as_ref().map(|value| value.block_height).into(),
                cursor.as_ref().map(|value| value.id).into(),
                voter_account_id.into(),
                vote_option.into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(|row| to_effective_vote_dto(row, proposal_id, &voting_options))
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
    path = "/api/v1/venear/delegates",
    summary = "List delegates",
    description = "Returns possible voters with own, delegated, and total voting power, ordered by account id ascending.",
    params(VenearDelegatesQuery),
    tag = "venear",
    operation_id = "listVenearDelegates",
    responses(
        (status = 200, description = "Delegate rows", body = VenearDelegatesListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_delegates(
    State(app_state): State<AppState>,
    Query(query): Query<VenearDelegatesQuery>,
) -> Result<Json<VenearDelegatesListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.limit);

    let delegate_cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<DelegateCursor>)
        .transpose()?
        .map(|cursor| cursor.account_id);

    let account_id_prefix = normalized_text_filter(query.account_id_prefix.as_deref());
    let has_inbound_delegation = query.has_inbound_delegation;
    let has_outbound_delegation = query.has_outbound_delegation;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                registered_voter_id AS account_id,
                own_voting_power,
                delegated_voting_power,
                total_voting_power,
                delegators_count,
                delegated_to_account_id,
                has_inbound_delegation,
                has_outbound_delegation,
                last_vote_ts,
                updated_ts
            FROM near.hos_registered_voters
            WHERE COALESCE(total_voting_power, '0')::numeric > 0
              AND ($1::text IS NULL OR registered_voter_id > $1::text)
              AND ($2::text IS NULL OR registered_voter_id LIKE ($2::text || '%'))
              AND ($3::boolean IS NULL OR has_inbound_delegation = $3::boolean)
              AND ($4::boolean IS NULL OR has_outbound_delegation = $4::boolean)
            ORDER BY registered_voter_id ASC
            LIMIT $5
            ",
            vec![
                delegate_cursor.into(),
                account_id_prefix.into(),
                has_inbound_delegation.into(),
                has_outbound_delegation.into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(to_delegate_dto)
        .collect::<Result<Vec<_>, _>>()?;

    let has_more = data.len() > limit as usize;
    if has_more {
        data.pop();
    }

    let next_cursor = data
        .last()
        .map(|row| {
            encode_cursor(&DelegateCursor {
                account_id: row.account_id.clone(),
            })
        })
        .transpose()?;

    Ok(Json(VenearDelegatesListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/delegates/{account_id}/voting-power",
    summary = "List voting power history for a delegate",
    description = "Returns voting power mutations for the given delegate/voter account.",
    params(
        ("account_id" = String, Path, description = "Delegate account id"),
        VenearDelegateVotingPowerQuery
    ),
    tag = "venear",
    operation_id = "listVenearDelegateVotingPower",
    responses(
        (status = 200, description = "Voting power history", body = VenearDelegateVotingPowerEventsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_delegate_voting_power(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
    Query(query): Query<VenearDelegateVotingPowerQuery>,
) -> Result<Json<VenearDelegateVotingPowerEventsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.limit);

    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<HeightIdCursor>)
        .transpose()?;

    let event_type = normalized_event_type_filter(query.event_type.as_deref());
    let proposal_id = query.proposal_id;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            WITH activity_events AS (
                SELECT
                    ROW_NUMBER() OVER (ORDER BY ua.block_timestamp DESC, ua.receipt_id DESC)::bigint AS id,
                    ua.event_type,
                    ua.proposal_id,
                    COALESCE(ua.voting_power, ua.near_amount, ua.locked_near_balance, 0)::text AS voting_power,
                    NULL::text AS delta_voting_power,
                    ua.block_height,
                    ua.block_timestamp,
                    ua.transaction_hash,
                    ua.receipt_id,
                    ua.execution_outcome_id,
                    d.event_type AS delegation_event_type,
                    d.delegator_account_id AS delegation_delegator_account_id
                FROM near.hos_user_activities ua
                LEFT JOIN near.hos_delegation_events d
                    ON d.receipt_id = ua.receipt_id
                WHERE ua.account_id = $1
            ),
            vote_events AS (
                SELECT
                    (ROW_NUMBER() OVER (ORDER BY v.block_timestamp DESC, v.receipt_id DESC)::bigint + 1000000000) AS id,
                    COALESCE(v.event_type, 'add_vote') AS event_type,
                    v.proposal_id::bigint AS proposal_id,
                    COALESCE(v.voting_power, 0)::text AS voting_power,
                    NULL::text AS delta_voting_power,
                    v.block_height,
                    v.block_timestamp,
                    v.transaction_hash,
                    v.receipt_id,
                    v.execution_outcome_id,
                    NULL::text AS delegation_event_type,
                    NULL::text AS delegation_delegator_account_id
                FROM near.hos_proposal_voting_history v
                WHERE v.voter_id = $1
            ),
            events AS (
                SELECT * FROM activity_events
                UNION ALL
                SELECT * FROM vote_events
            )
            SELECT
                id,
                event_type,
                proposal_id,
                voting_power,
                delta_voting_power,
                block_height,
                block_timestamp,
                transaction_hash,
                receipt_id,
                execution_outcome_id,
                delegation_event_type,
                delegation_delegator_account_id
            FROM events
            WHERE 1 = 1
              AND (
                $2::bigint IS NULL
                OR block_height < $2::bigint
                OR (block_height = $2::bigint AND id < $3::bigint)
              )
              AND ($4::text IS NULL OR event_type = $4::text)
              AND ($5::bigint IS NULL OR proposal_id = $5::bigint)
            ORDER BY block_height DESC, id DESC
            LIMIT $6
            ",
            vec![
                account_id.clone().into(),
                cursor.as_ref().map(|value| value.block_height).into(),
                cursor.as_ref().map(|value| value.id).into(),
                event_type.into(),
                proposal_id.into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(|row| to_delegate_voting_power_dto(row, &account_id))
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

    Ok(Json(VenearDelegateVotingPowerEventsListResponse {
        data,
        next_cursor,
        has_more,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/venear/delegates/{account_id}/delegations",
    summary = "List delegation events for a delegate",
    description = "Returns delegation events where the account appears as delegator or delegatee, with delegatee inference for undelegate events when possible.",
    params(
        ("account_id" = String, Path, description = "Delegate account id"),
        VenearDelegateDelegationsQuery
    ),
    tag = "venear",
    operation_id = "listVenearDelegateDelegations",
    responses(
        (status = 200, description = "Delegation events", body = VenearDelegationEventsListResponse),
        (status = 400, description = "Invalid request", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_delegate_delegations(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
    Query(query): Query<VenearDelegateDelegationsQuery>,
) -> Result<Json<VenearDelegationEventsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = resolve_limit(query.limit);

    let cursor = query
        .cursor
        .as_deref()
        .map(decode_cursor::<HeightIdCursor>)
        .transpose()?;

    let event_type = normalized_event_type_filter(query.event_type.as_deref());
    let direction = parse_delegation_direction_filter(query.direction.as_deref())?;
    let counterparty_account_id = normalized_text_filter(query.counterparty_account_id.as_deref());
    let success = query.success;

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            WITH base_events AS (
                SELECT
                    ROW_NUMBER() OVER (
                        ORDER BY e.block_timestamp ASC, e.receipt_id ASC, e.id ASC
                    )::bigint AS id,
                    e.delegator_account_id,
                    e.delegatee_account_id,
                    e.event_type,
                    e.success,
                    e.block_height,
                    e.block_timestamp,
                    e.transaction_hash,
                    e.receipt_id,
                    e.execution_outcome_id
                FROM near.hos_delegation_events e
            ),
            events AS (
                SELECT
                    e.id,
                    e.delegator_account_id,
                    e.delegatee_account_id,
                    e.event_type,
                    e.success,
                    e.block_height,
                    e.block_timestamp,
                    e.transaction_hash,
                    e.receipt_id,
                    e.execution_outcome_id,
                    prev.delegatee_account_id AS inferred_delegatee_account_id
                FROM base_events e
                LEFT JOIN LATERAL (
                    SELECT p.delegatee_account_id
                    FROM base_events p
                    WHERE p.delegator_account_id = e.delegator_account_id
                      AND p.event_type = 'delegate_all'
                      AND p.success = true
                      AND (
                        p.block_height < e.block_height
                        OR (p.block_height = e.block_height AND p.id < e.id)
                      )
                    ORDER BY p.block_height DESC, p.id DESC
                    LIMIT 1
                ) prev ON TRUE
            )
            SELECT
                id,
                delegator_account_id,
                delegatee_account_id,
                event_type,
                success,
                block_height,
                block_timestamp,
                transaction_hash,
                receipt_id,
                execution_outcome_id,
                COALESCE(delegatee_account_id, CASE WHEN event_type = 'undelegate' THEN inferred_delegatee_account_id END) AS resolved_delegatee_account_id,
                (delegatee_account_id IS NULL AND event_type = 'undelegate' AND inferred_delegatee_account_id IS NOT NULL) AS delegatee_inferred
            FROM events
            WHERE (
                delegator_account_id = $1
                OR COALESCE(delegatee_account_id, CASE WHEN event_type = 'undelegate' THEN inferred_delegatee_account_id END) = $1
            )
              AND (
                $2::bigint IS NULL
                OR block_height < $2::bigint
                OR (block_height = $2::bigint AND id < $3::bigint)
              )
              AND ($4::text IS NULL OR event_type = $4::text)
              AND (
                $5::text IS NULL
                OR ($5::text = 'outbound' AND delegator_account_id = $1)
                OR (
                    $5::text = 'inbound'
                    AND COALESCE(delegatee_account_id, CASE WHEN event_type = 'undelegate' THEN inferred_delegatee_account_id END) = $1
                )
              )
              AND (
                $6::text IS NULL
                OR (
                    delegator_account_id = $1
                    AND COALESCE(delegatee_account_id, CASE WHEN event_type = 'undelegate' THEN inferred_delegatee_account_id END) = $6::text
                )
                OR (
                    COALESCE(delegatee_account_id, CASE WHEN event_type = 'undelegate' THEN inferred_delegatee_account_id END) = $1
                    AND delegator_account_id = $6::text
                )
              )
              AND ($7::boolean IS NULL OR success = $7::boolean)
            ORDER BY block_height DESC, id DESC
            LIMIT $8
            ",
            vec![
                account_id.clone().into(),
                cursor.as_ref().map(|value| value.block_height).into(),
                cursor.as_ref().map(|value| value.id).into(),
                event_type.into(),
                direction.into(),
                counterparty_account_id.into(),
                success.into(),
                i64::try_from(limit + 1).unwrap_or(101).into(),
            ],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    let mut data = rows
        .iter()
        .map(|row| to_delegation_event_dto(row, &account_id))
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

#[allow(clippy::result_large_err)]
fn to_proposal_summary_dto(
    row: &QueryResult,
) -> Result<VenearProposalSummaryDto, ApiErrorResponse> {
    let proposal_id = decode_row_col::<i64>(row, "proposal_id", "proposal")?;
    let latest_event_type = decode_row_col::<String>(row, "event_type", "proposal")?;
    let proposer_id = decode_row_col::<Option<String>>(row, "proposer_id", "proposal")?;
    let create_proposer_id =
        decode_row_col::<Option<String>>(row, "create_proposer_id", "proposal")?;
    let reviewer_id = decode_row_col::<Option<String>>(row, "reviewer_id", "proposal")?;
    let voting_start_time_sec =
        decode_row_col::<Option<i64>>(row, "voting_start_time_sec", "proposal")?;
    let snapshot_block_height =
        decode_row_col::<Option<i64>>(row, "snapshot_block_height", "proposal")?;
    let snapshot_root = decode_row_col::<Option<String>>(row, "snapshot_root", "proposal")?;
    let snapshot_total_venear =
        decode_row_col::<Option<String>>(row, "snapshot_total_venear", "proposal")?;
    let metadata = decode_row_col::<Option<JsonValue>>(row, "metadata", "proposal")?;
    let create_metadata = decode_row_col::<Option<JsonValue>>(row, "create_metadata", "proposal")?;
    let payload = decode_row_col::<Option<JsonValue>>(row, "payload", "proposal")?;
    let block_height = decode_row_col::<i64>(row, "block_height", "proposal")?;
    let block_timestamp_ns = decode_row_col::<i64>(row, "block_timestamp", "proposal")?;
    let created_block_timestamp_ns =
        decode_row_col::<Option<i64>>(row, "created_block_timestamp", "proposal")?;
    let effective_votes_count = decode_row_col::<i64>(row, "effective_votes_count", "proposal")?;
    let effective_votes_voting_power =
        decode_row_col::<String>(row, "effective_votes_voting_power", "proposal")?;

    let sources = metadata_sources(
        metadata.as_ref(),
        create_metadata.as_ref(),
        payload.as_ref(),
    );
    let title = pick_string_from_sources(&sources, "title");
    let description = pick_string_from_sources(&sources, "description");
    let link = pick_string_from_sources(&sources, "link");
    let voting_options = pick_voting_options_from_sources(&sources);

    let voting_start_ns = proposal_voting_start_ns(payload.as_ref(), voting_start_time_sec);
    let voting_duration_ns = proposal_voting_duration_ns(payload.as_ref());
    let created_at_ns = proposal_created_at_ns(payload.as_ref(), created_block_timestamp_ns);

    let voting_start_at = voting_start_ns.map(ns_to_rfc3339).transpose()?;
    let voting_end_at = match (voting_start_ns, voting_duration_ns) {
        (Some(start_ns), Some(duration_ns)) => start_ns
            .checked_add(duration_ns)
            .map(ns_to_rfc3339)
            .transpose()?,
        _ => None,
    };

    let status = derive_proposal_status(
        &latest_event_type,
        metadata.as_ref(),
        payload.as_ref(),
        voting_start_ns,
        voting_duration_ns,
    );

    Ok(VenearProposalSummaryDto {
        proposal_id,
        title,
        description,
        link,
        voting_options,
        status,
        proposer_id: proposer_id.or(create_proposer_id),
        reviewer_id,
        created_at: created_at_ns.map(ns_to_rfc3339).transpose()?,
        voting_start_at,
        voting_end_at,
        snapshot_block_height,
        snapshot_root,
        snapshot_total_venear,
        effective_votes_count,
        effective_votes_voting_power,
        latest_event_type,
        latest_block_height: block_height,
        latest_block_timestamp: ns_to_rfc3339(block_timestamp_ns)?,
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
    let metadata = decode_row_col::<Option<JsonValue>>(row, "metadata", "proposal event")?;
    let payload = decode_row_col::<Option<JsonValue>>(row, "payload", "proposal event")?;
    let block_height = decode_row_col::<i64>(row, "block_height", "proposal event")?;
    let block_timestamp_ns = decode_row_col::<i64>(row, "block_timestamp", "proposal event")?;
    let transaction_hash =
        decode_row_col::<Option<String>>(row, "transaction_hash", "proposal event")?;
    let receipt_id = decode_row_col::<String>(row, "receipt_id", "proposal event")?;
    let execution_outcome_id =
        decode_row_col::<String>(row, "execution_outcome_id", "proposal event")?;

    let sources = metadata_sources(metadata.as_ref(), None, payload.as_ref());
    let voting_start_ns = proposal_voting_start_ns(payload.as_ref(), voting_start_time_sec);
    let voting_duration_ns = proposal_voting_duration_ns(payload.as_ref());

    Ok(VenearProposalEventDto {
        id,
        proposal_id,
        event_type: event_type.clone(),
        actor_account_id: account_id
            .or_else(|| proposer_id.clone())
            .or_else(|| reviewer_id.clone()),
        proposer_id,
        reviewer_id,
        status: Some(derive_proposal_status(
            &event_type,
            metadata.as_ref(),
            payload.as_ref(),
            voting_start_ns,
            voting_duration_ns,
        )),
        title: pick_string_from_sources(&sources, "title"),
        description: pick_string_from_sources(&sources, "description"),
        link: pick_string_from_sources(&sources, "link"),
        voting_start_at: voting_start_ns.map(ns_to_rfc3339).transpose()?,
        voting_duration_ns: voting_duration_ns.map(|value| value.to_string()),
        snapshot_block_height,
        snapshot_root,
        snapshot_total_venear,
        block_height,
        block_timestamp: ns_to_rfc3339(block_timestamp_ns)?,
        transaction_hash,
        receipt_id,
        execution_outcome_id,
    })
}

#[allow(clippy::result_large_err)]
fn to_effective_vote_dto(
    row: &QueryResult,
    proposal_id: i64,
    voting_options: &[String],
) -> Result<VenearVoteDto, ApiErrorResponse> {
    let id = decode_row_col::<i64>(row, "id", "vote")?;
    let voter_account_id = decode_row_col::<String>(row, "account_id", "vote")?;
    let vote_option = decode_row_col::<i32>(row, "vote", "vote")?;
    let voting_power = decode_row_col::<String>(row, "account_balance", "vote")?;
    let event_type = decode_row_col::<String>(row, "event_type", "vote")?;
    let block_height = decode_row_col::<i64>(row, "block_height", "vote")?;
    let block_timestamp_ns = decode_row_col::<i64>(row, "block_timestamp", "vote")?;
    let transaction_hash = decode_row_col::<Option<String>>(row, "transaction_hash", "vote")?;
    let receipt_id = decode_row_col::<String>(row, "receipt_id", "vote")?;
    let execution_outcome_id = decode_row_col::<String>(row, "execution_outcome_id", "vote")?;

    let vote_label = usize::try_from(vote_option)
        .ok()
        .and_then(|idx| voting_options.get(idx).cloned());

    Ok(VenearVoteDto {
        id,
        proposal_id,
        voter_account_id,
        vote_option,
        vote_label,
        voting_power,
        event_type: event_type.clone(),
        is_effective: event_type == "add_vote",
        block_height,
        block_timestamp: ns_to_rfc3339(block_timestamp_ns)?,
        transaction_hash,
        receipt_id,
        execution_outcome_id,
    })
}

#[allow(clippy::result_large_err)]
fn to_delegate_dto(row: &QueryResult) -> Result<VenearDelegateDto, ApiErrorResponse> {
    let account_id = decode_row_col::<String>(row, "account_id", "delegate")?;
    let own_voting_power = decode_row_col::<String>(row, "own_voting_power", "delegate")?;
    let delegated_voting_power =
        decode_row_col::<String>(row, "delegated_voting_power", "delegate")?;
    let total_voting_power = decode_row_col::<String>(row, "total_voting_power", "delegate")?;
    let delegators_count = decode_row_col::<i64>(row, "delegators_count", "delegate")?;
    let delegated_to_account_id =
        decode_row_col::<Option<String>>(row, "delegated_to_account_id", "delegate")?;
    let has_inbound_delegation = decode_row_col::<bool>(row, "has_inbound_delegation", "delegate")?;
    let has_outbound_delegation =
        decode_row_col::<bool>(row, "has_outbound_delegation", "delegate")?;
    let last_vote_ts = decode_row_col::<Option<i64>>(row, "last_vote_ts", "delegate")?;
    let updated_ts = decode_row_col::<i64>(row, "updated_ts", "delegate")?;

    Ok(VenearDelegateDto {
        account_id,
        own_voting_power,
        delegated_voting_power,
        total_voting_power,
        delegators_count,
        delegated_to_account_id,
        has_inbound_delegation,
        has_outbound_delegation,
        last_vote_at: last_vote_ts.map(ns_to_rfc3339).transpose()?,
        updated_at: ns_to_rfc3339(updated_ts)?,
    })
}

#[allow(clippy::result_large_err)]
fn to_delegate_voting_power_dto(
    row: &QueryResult,
    account_id: &str,
) -> Result<VenearDelegateVotingPowerEventDto, ApiErrorResponse> {
    let id = decode_row_col::<i64>(row, "id", "delegate voting power")?;
    let event_type = decode_row_col::<String>(row, "event_type", "delegate voting power")?;
    let proposal_id = decode_row_col::<Option<i64>>(row, "proposal_id", "delegate voting power")?;
    let voting_power = decode_row_col::<String>(row, "voting_power", "delegate voting power")?;
    let delta_voting_power =
        decode_row_col::<Option<String>>(row, "delta_voting_power", "delegate voting power")?;
    let block_height = decode_row_col::<i64>(row, "block_height", "delegate voting power")?;
    let block_timestamp_ns =
        decode_row_col::<i64>(row, "block_timestamp", "delegate voting power")?;
    let transaction_hash =
        decode_row_col::<Option<String>>(row, "transaction_hash", "delegate voting power")?;
    let receipt_id = decode_row_col::<String>(row, "receipt_id", "delegate voting power")?;
    let execution_outcome_id =
        decode_row_col::<String>(row, "execution_outcome_id", "delegate voting power")?;
    let delegation_event_type =
        decode_row_col::<Option<String>>(row, "delegation_event_type", "delegate voting power")?;
    let delegation_delegator_account_id = decode_row_col::<Option<String>>(
        row,
        "delegation_delegator_account_id",
        "delegate voting power",
    )?;
    let is_delegator = delegation_delegator_account_id
        .as_deref()
        .is_some_and(|d| d == account_id);

    Ok(VenearDelegateVotingPowerEventDto {
        id,
        account_id: account_id.to_string(),
        event_type: event_type.clone(),
        reason: voting_power_reason(&event_type, delegation_event_type.as_deref(), is_delegator)
            .to_string(),
        proposal_id,
        voting_power,
        delta_voting_power,
        block_height,
        block_timestamp: ns_to_rfc3339(block_timestamp_ns)?,
        transaction_hash,
        receipt_id,
        execution_outcome_id,
    })
}

#[allow(clippy::result_large_err)]
fn to_delegation_event_dto(
    row: &QueryResult,
    account_id: &str,
) -> Result<VenearDelegationEventDto, ApiErrorResponse> {
    let id = decode_row_col::<i64>(row, "id", "delegation event")?;
    let delegator_account_id =
        decode_row_col::<String>(row, "delegator_account_id", "delegation event")?;
    let delegatee_account_id =
        decode_row_col::<Option<String>>(row, "delegatee_account_id", "delegation event")?;
    let resolved_delegatee_account_id =
        decode_row_col::<Option<String>>(row, "resolved_delegatee_account_id", "delegation event")?;
    let delegatee_inferred = decode_row_col::<bool>(row, "delegatee_inferred", "delegation event")?;
    let event_type = decode_row_col::<String>(row, "event_type", "delegation event")?;
    let success = decode_row_col::<bool>(row, "success", "delegation event")?;
    let block_height = decode_row_col::<i64>(row, "block_height", "delegation event")?;
    let block_timestamp_ns = decode_row_col::<i64>(row, "block_timestamp", "delegation event")?;
    let transaction_hash =
        decode_row_col::<Option<String>>(row, "transaction_hash", "delegation event")?;
    let receipt_id = decode_row_col::<String>(row, "receipt_id", "delegation event")?;
    let execution_outcome_id =
        decode_row_col::<String>(row, "execution_outcome_id", "delegation event")?;

    let direction = if delegator_account_id == account_id {
        "outbound"
    } else {
        "inbound"
    };

    Ok(VenearDelegationEventDto {
        id,
        account_id: account_id.to_string(),
        direction: direction.to_string(),
        event_type,
        delegator_account_id,
        delegatee_account_id,
        resolved_delegatee_account_id,
        delegatee_inferred,
        success,
        block_height,
        block_timestamp: ns_to_rfc3339(block_timestamp_ns)?,
        transaction_hash,
        receipt_id,
        execution_outcome_id,
    })
}

struct ConfigEntryWithCursor {
    dto: VenearConfigEntryDto,
    cursor: ConfigCursor,
}

#[allow(clippy::result_large_err)]
fn to_config_entry_dto(row: &QueryResult) -> Result<VenearConfigEntryDto, ApiErrorResponse> {
    let id = decode_row_col::<i64>(row, "id", "config event")?;
    let contract_id = decode_row_col::<String>(row, "contract_id", "config event")?;
    let method_name = decode_row_col::<String>(row, "method_name", "config event")?;
    let predecessor_account_id =
        decode_row_col::<String>(row, "predecessor_account_id", "config event")?;
    let transaction_hash =
        decode_row_col::<Option<String>>(row, "transaction_hash", "config event")?;
    let receipt_id = decode_row_col::<String>(row, "receipt_id", "config event")?;
    let execution_outcome_id =
        decode_row_col::<String>(row, "execution_outcome_id", "config event")?;
    let block_height = decode_row_col::<i64>(row, "block_height", "config event")?;
    let block_timestamp_ns = decode_row_col::<i64>(row, "block_timestamp", "config event")?;
    let field = decode_row_col::<String>(row, "config_field", "config event")?;
    let source = decode_row_col::<String>(row, "config_source", "config event")?;
    let config_value = decode_row_col::<JsonValue>(row, "config_value", "config event")?;

    Ok(VenearConfigEntryDto {
        id,
        contract_id,
        field,
        value: json_value_to_string_vec(&config_value),
        source,
        method_name,
        predecessor_account_id,
        transaction_hash,
        receipt_id,
        execution_outcome_id,
        block_height,
        block_timestamp: ns_to_rfc3339(block_timestamp_ns)?,
    })
}

#[allow(clippy::result_large_err)]
fn to_config_entry_with_cursor(
    row: &QueryResult,
) -> Result<ConfigEntryWithCursor, ApiErrorResponse> {
    let dto = to_config_entry_dto(row)?;
    let field_order = decode_row_col::<i32>(row, "field_order", "config event")?;

    Ok(ConfigEntryWithCursor {
        cursor: ConfigCursor {
            block_height: dto.block_height,
            id: dto.id,
            field_order,
        },
        dto,
    })
}

async fn fetch_proposal_voting_options(
    db: &DatabaseConnection,
    proposal_id: i64,
) -> Result<Vec<String>, ApiErrorResponse> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                voting_options,
                proposal_metadata AS metadata,
                proposal_payload AS payload
            FROM near.hos_proposals
            WHERE proposal_id = $1
            LIMIT 1
            ",
            vec![proposal_id.into()],
        ))
        .await
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::not_found("Proposal not found"))?;

    let voting_options = decode_row_col::<Option<Vec<String>>>(&row, "voting_options", "proposal")?
        .unwrap_or_default();
    if !voting_options.is_empty() {
        return Ok(voting_options);
    }

    let metadata = decode_row_col::<Option<JsonValue>>(&row, "metadata", "proposal")?;
    let payload = decode_row_col::<Option<JsonValue>>(&row, "payload", "proposal")?;

    let sources = metadata_sources(metadata.as_ref(), None, payload.as_ref());
    Ok(pick_voting_options_from_sources(&sources))
}

async fn fetch_vote_aggregates(
    db: &DatabaseConnection,
    proposal_id: i64,
    voting_options: &[String],
) -> Result<Vec<VenearVoteAggregateDto>, ApiErrorResponse> {
    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            WITH latest_votes AS (
                SELECT
                    voter_id AS account_id,
                    COALESCE(vote_option, 0)::integer AS vote,
                    COALESCE(voting_power, 0)::numeric AS account_balance,
                    event_type
                FROM near.hos_proposal_voting_history
                WHERE proposal_id = $1
            )
            SELECT
                vote::integer AS option_index,
                COUNT(*)::bigint AS votes_count,
                COALESCE(SUM(account_balance), 0)::text AS voting_power
            FROM latest_votes
            WHERE event_type = 'add_vote'
            GROUP BY vote
            ORDER BY vote ASC
            ",
            vec![proposal_id.into()],
        ))
        .await
        .map_err(ApiErrorResponse::from)?;

    rows.iter()
        .map(|row| {
            let option_index = decode_row_col::<i32>(row, "option_index", "vote aggregate")?;
            let votes_count = decode_row_col::<i64>(row, "votes_count", "vote aggregate")?;
            let voting_power = decode_row_col::<String>(row, "voting_power", "vote aggregate")?;
            let option_label = usize::try_from(option_index)
                .ok()
                .and_then(|idx| voting_options.get(idx).cloned());

            Ok(VenearVoteAggregateDto {
                option_index,
                option_label,
                votes_count,
                voting_power,
            })
        })
        .collect::<Result<Vec<_>, ApiErrorResponse>>()
}

async fn ensure_proposal_exists(
    db: &DatabaseConnection,
    proposal_id: i64,
) -> Result<(), ApiErrorResponse> {
    let count = scalar_i64_with_values(
        db,
        "SELECT COUNT(*)::bigint AS value FROM near.hos_proposals WHERE proposal_id = $1",
        vec![proposal_id.into()],
    )
    .await?;

    if count > 0 {
        Ok(())
    } else {
        Err(ApiErrorResponse::not_found("Proposal not found"))
    }
}

fn metadata_sources<'a>(
    metadata: Option<&'a JsonValue>,
    create_metadata: Option<&'a JsonValue>,
    payload: Option<&'a JsonValue>,
) -> [Option<&'a JsonValue>; 3] {
    [
        metadata,
        create_metadata,
        payload.and_then(extract_success_value),
    ]
}

fn extract_success_value(payload: &JsonValue) -> Option<&JsonValue> {
    payload.get("success_value")
}

fn pick_string_from_sources(sources: &[Option<&JsonValue>], key: &str) -> Option<String> {
    sources.iter().flatten().find_map(|source| {
        source
            .get(key)
            .and_then(json_value_to_string)
            .filter(|v| !v.is_empty())
    })
}

fn pick_voting_options_from_sources(sources: &[Option<&JsonValue>]) -> Vec<String> {
    for source in sources.iter().flatten() {
        if let Some(value) = source.get("voting_options") {
            let options = json_value_to_string_vec(value);
            if !options.is_empty() {
                return options;
            }
        }
    }
    Vec::new()
}

fn proposal_voting_start_ns(
    payload: Option<&JsonValue>,
    voting_start_time_sec: Option<i64>,
) -> Option<i64> {
    payload
        .and_then(extract_success_value)
        .and_then(|value| value.get("voting_start_time_ns"))
        .and_then(json_value_to_i64)
        .or_else(|| voting_start_time_sec.and_then(|sec| sec.checked_mul(1_000_000_000)))
}

fn proposal_voting_duration_ns(payload: Option<&JsonValue>) -> Option<i64> {
    payload
        .and_then(extract_success_value)
        .and_then(|value| value.get("voting_duration_ns"))
        .and_then(json_value_to_i64)
}

fn proposal_created_at_ns(
    payload: Option<&JsonValue>,
    created_block_timestamp_ns: Option<i64>,
) -> Option<i64> {
    payload
        .and_then(extract_success_value)
        .and_then(|value| value.get("creation_time_ns"))
        .and_then(json_value_to_i64)
        .or(created_block_timestamp_ns)
}

fn derive_proposal_status(
    event_type: &str,
    metadata: Option<&JsonValue>,
    payload: Option<&JsonValue>,
    voting_start_ns: Option<i64>,
    voting_duration_ns: Option<i64>,
) -> String {
    match event_type {
        "proposal_reject" => "rejected".to_string(),
        "create_proposal" => "created".to_string(),
        "proposal_approve" => "approved".to_string(),
        "proposal_approve_finalized" => {
            if let (Some(start_ns), Some(duration_ns)) = (voting_start_ns, voting_duration_ns) {
                if let Some(end_ns) = start_ns.checked_add(duration_ns) {
                    let now_ns = Utc::now().timestamp_micros().saturating_mul(1_000);
                    if now_ns >= end_ns {
                        return "finished".to_string();
                    }
                    if now_ns >= start_ns {
                        return "voting".to_string();
                    }
                }
            }
            "approved".to_string()
        }
        _ => {
            let raw_status = metadata
                .and_then(|value| value.get("status"))
                .and_then(json_value_to_string)
                .or_else(|| {
                    payload
                        .and_then(extract_success_value)
                        .and_then(|value| value.get("status"))
                        .and_then(json_value_to_string)
                });

            raw_status
                .as_deref()
                .and_then(normalize_proposal_status)
                .unwrap_or_else(|| "created".to_string())
        }
    }
}

fn normalize_proposal_status(raw: &str) -> Option<String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "created" => Some("created".to_string()),
        "rejected" => Some("rejected".to_string()),
        "approved" => Some("approved".to_string()),
        "voting" => Some("voting".to_string()),
        "finished" => Some("finished".to_string()),
        _ => None,
    }
}

fn voting_power_reason(
    event_type: &str,
    delegation_event_type: Option<&str>,
    is_delegator: bool,
) -> &'static str {
    match (event_type, delegation_event_type) {
        ("ft_mint", Some("delegate_all")) => {
            if is_delegator {
                "balance_increase"
            } else {
                "delegation_received"
            }
        }
        ("ft_burn", Some("delegate_all")) => {
            if is_delegator {
                "delegation_given"
            } else {
                "balance_decrease"
            }
        }
        ("ft_mint", Some("undelegate")) => {
            if is_delegator {
                "delegation_returned"
            } else {
                "balance_increase"
            }
        }
        ("ft_burn", Some("undelegate")) => {
            if is_delegator {
                "balance_decrease"
            } else {
                "delegation_removed"
            }
        }
        ("ft_mint", _) => "balance_increase",
        ("ft_burn", _) => "balance_decrease",
        ("add_vote", _) => "vote_cast",
        ("remove_vote", _) => "vote_removed",
        _ => "other",
    }
}

fn json_value_to_string(value: &JsonValue) -> Option<String> {
    match value {
        JsonValue::String(text) => Some(text.clone()),
        JsonValue::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn json_value_to_i64(value: &JsonValue) -> Option<i64> {
    match value {
        JsonValue::Number(number) => number.as_i64(),
        JsonValue::String(text) => text.parse::<i64>().ok(),
        _ => None,
    }
}

fn json_value_to_string_vec(value: &JsonValue) -> Vec<String> {
    let Some(array) = value.as_array() else {
        return Vec::new();
    };

    array
        .iter()
        .filter_map(json_value_to_string)
        .filter(|entry| !entry.is_empty())
        .collect()
}

fn normalized_text_filter(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn normalized_contract_id_filter(value: Option<&str>) -> Option<String> {
    normalized_text_filter(value).map(|value| value.to_ascii_lowercase())
}

fn normalized_event_type_filter(value: Option<&str>) -> Option<String> {
    normalized_text_filter(value).map(|value| value.to_ascii_lowercase())
}

#[allow(clippy::result_large_err)]
fn parse_config_field_filter(value: Option<&str>) -> Result<Option<String>, ApiErrorResponse> {
    let Some(field) = normalized_text_filter(value) else {
        return Ok(None);
    };

    match field.to_ascii_lowercase().as_str() {
        "guardians" => Ok(Some("guardians".to_string())),
        "reviewer_ids" => Ok(Some("reviewer_ids".to_string())),
        _ => Err(ApiErrorResponse::bad_request(
            "Invalid field filter. Use 'guardians' or 'reviewer_ids'.",
        )),
    }
}

#[allow(clippy::result_large_err)]
fn parse_delegation_direction_filter(
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
    fn status_normalization_is_expected() {
        assert_eq!(
            normalize_proposal_status("Created"),
            Some("created".to_string())
        );
        assert_eq!(
            normalize_proposal_status("Voting"),
            Some("voting".to_string())
        );
        assert_eq!(normalize_proposal_status("unknown"), None);
    }

    #[test]
    fn finalized_status_prefers_time_window_over_stale_metadata() {
        let metadata = serde_json::json!({"status":"Voting"});
        let status = derive_proposal_status(
            "proposal_approve_finalized",
            Some(&metadata),
            None,
            Some(1),
            Some(1),
        );
        assert_eq!(status, "finished");
    }

    #[test]
    fn timestamp_conversion_works() {
        assert_eq!(
            ns_to_rfc3339(1_760_000_000_000_000_000).expect("ns to rfc3339"),
            "2025-10-09T08:53:20+00:00"
        );
    }

    #[test]
    fn text_filter_normalization_trims_and_ignores_empty_values() {
        assert_eq!(normalized_text_filter(None), None);
        assert_eq!(normalized_text_filter(Some("   ")), None);
        assert_eq!(
            normalized_text_filter(Some(" lane.near ")),
            Some("lane.near".to_string())
        );
    }

    #[test]
    fn delegation_direction_filter_is_validated() {
        assert_eq!(
            parse_delegation_direction_filter(Some("inbound")).expect("inbound direction"),
            Some("inbound".to_string())
        );
        assert_eq!(
            parse_delegation_direction_filter(Some(" OUTBOUND ")).expect("outbound direction"),
            Some("outbound".to_string())
        );
        assert!(parse_delegation_direction_filter(Some("sideways")).is_err());
    }

    #[test]
    fn config_field_filter_is_validated() {
        assert_eq!(
            parse_config_field_filter(Some("guardians")).expect("guardians field"),
            Some("guardians".to_string())
        );
        assert_eq!(
            parse_config_field_filter(Some(" REVIEWER_IDS ")).expect("reviewer ids field"),
            Some("reviewer_ids".to_string())
        );
        assert!(parse_config_field_filter(Some("owners")).is_err());
    }
}
