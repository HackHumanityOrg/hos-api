//! Legacy Agora compatibility handlers backed by local HOS materialized views.

use crate::{state::AppState, types::*};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use chrono::{NaiveDate, NaiveDateTime, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, QueryResult, Statement, TransactionTrait,
    Value,
};
use serde::Serialize;
use std::{env, sync::Arc};
use tracing::error;

const DEFAULT_PAGE_SIZE: i64 = 10;
const DEFAULT_PAGE_NUMBER: i64 = 1;
const DEFAULT_QUORUM_PERCENTAGE_BPS: &str = "3500";
const PROD_QUORUM_FLOOR: &str = "7000000000000000000000000000000";
const NON_PROD_QUORUM_FLOOR: &str = "10000000000000000000000000";
const METADATA_PREFIX: &str = "\u{001E}\u{001E}\u{001E}\u{001E}";
const METADATA_VERSION: &str = "\u{0001}\u{0001}";
const SIMPLE_MAJORITY_BPS: i64 = 5000;
const SUPER_MAJORITY_BPS: i64 = 6667;
const DELEGATE_NOT_FOUND_MESSAGE: &str =
    "Address not found in registered voters and is not a valid NEAR account";
const DATE_TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

#[derive(Debug, Clone, Copy)]
enum ProposalType {
    SimpleMajority,
    SuperMajority,
}

impl ProposalType {
    fn as_str(self) -> &'static str {
        match self {
            Self::SimpleMajority => "SimpleMajority",
            Self::SuperMajority => "SuperMajority",
        }
    }
}

#[derive(Debug, Clone)]
struct ProposalMetadata {
    proposal_type: ProposalType,
    quorum: String,
    approval_threshold: i64,
}

#[derive(Debug, Clone, Copy)]
struct DecimalValue {
    coefficient: i128,
    scale: u32,
}

#[utoipa::path(
    get,
    path = "/api/agora/proposal/approved",
    summary = "List approved Agora proposals",
    description = "Returns approved, non-rejected proposals in the legacy Agora response shape.",
    params(AgoraPaginationQuery),
    tag = "agora",
    operation_id = "getAgoraApprovedProposals",
    responses(
        (status = 200, description = "Approved proposal rows", body = AgoraProposalListResponse),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_approved_proposals(
    State(app_state): State<AppState>,
    Query(query): Query<AgoraPaginationQuery>,
) -> Response {
    let db = match require_db(&app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    let (page_size, _page_number, offset) =
        legacy_page_params(query.page_size.as_deref(), query.page.as_deref());

    match fetch_legacy_proposal_list(db.as_ref(), Some(true), None, page_size, offset).await {
        Ok(response) => json_ok(response),
        Err(err) => {
            error!(error = %err, "failed to fetch approved Agora proposals");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch approved proposals",
            )
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/agora/proposal/pending",
    summary = "List pending Agora proposals",
    description = "Returns unapproved, unrejected proposals in the legacy Agora response shape.",
    params(AgoraPendingProposalsQuery),
    tag = "agora",
    operation_id = "getAgoraPendingProposals",
    responses(
        (status = 200, description = "Pending proposal rows", body = AgoraProposalListResponse),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_pending_proposals(
    State(app_state): State<AppState>,
    Query(query): Query<AgoraPendingProposalsQuery>,
) -> Response {
    let db = match require_db(&app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    let (page_size, _page_number, offset) =
        legacy_page_params(query.page_size.as_deref(), query.page.as_deref());

    match fetch_legacy_proposal_list(
        db.as_ref(),
        Some(false),
        normalized_text(query.created_by.as_deref()),
        page_size,
        offset,
    )
    .await
    {
        Ok(response) => json_ok(response),
        Err(err) => {
            error!(error = %err, "failed to fetch pending Agora proposals");
            json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch pending proposals",
            )
        }
    }
}

#[utoipa::path(
    get,
    path = "/api/agora/proposal/{proposal_id}/votes",
    summary = "List proposal votes",
    description = "Returns latest proposal-vote rows using the legacy Agora payload shape.",
    params(
        ("proposal_id" = String, Path, description = "Proposal id"),
        AgoraPaginationQuery
    ),
    tag = "agora",
    operation_id = "getAgoraProposalVotes",
    responses(
        (status = 200, description = "Proposal vote rows", body = AgoraProposalVotesResponse),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_proposal_votes(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<String>,
    Query(query): Query<AgoraPaginationQuery>,
) -> Response {
    let db = match require_db(&app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    let Some(proposal_id) = parse_js_int(proposal_id.as_str()) else {
        return json_ok(AgoraProposalVotesResponse {
            votes: Vec::new(),
            count: 0,
        });
    };

    let (page_size, _page_number, offset) =
        legacy_page_params(query.page_size.as_deref(), query.page.as_deref());

    let rows = match db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                voter_id,
                vote_option::text AS vote_option,
                voting_power::text AS voting_power_text
            FROM near.hos_proposal_voting_history
            WHERE proposal_id = $1
            ORDER BY voting_power DESC NULLS LAST, voter_id ASC
            LIMIT $2 OFFSET $3
            ",
            vec![proposal_id.into(), page_size.into(), offset.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            error!(error = %err, "failed to query Agora proposal votes");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal voting history",
            );
        }
    };

    let count = match scalar_i64_with_values(
        db.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.hos_proposal_voting_history WHERE proposal_id = $1",
        vec![proposal_id.into()],
    )
    .await
    {
        Ok(count) => count,
        Err(err) => {
            error!(error = %err, "failed to count Agora proposal votes");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal voting history",
            );
        }
    };

    let votes = match rows
        .iter()
        .map(to_agora_proposal_vote_dto)
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(votes) => votes,
        Err(err) => {
            error!(error = %err, "failed to decode Agora proposal votes");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal voting history",
            );
        }
    };

    json_ok(AgoraProposalVotesResponse { votes, count })
}

#[utoipa::path(
    get,
    path = "/api/agora/proposal/{proposal_id}/non-voters",
    summary = "List proposal non-voters",
    description = "Returns non-voters from the HOS compatibility view using the legacy Agora payload shape.",
    params(
        ("proposal_id" = String, Path, description = "Proposal id"),
        AgoraPaginationQuery
    ),
    tag = "agora",
    operation_id = "getAgoraProposalNonVoters",
    responses(
        (status = 200, description = "Proposal non-voter rows", body = AgoraProposalNonVotersResponse),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_proposal_non_voters(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<String>,
    Query(query): Query<AgoraPaginationQuery>,
) -> Response {
    let db = match require_db(&app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    let Some(proposal_id) = parse_js_int(proposal_id.as_str()) else {
        return json_ok(AgoraProposalNonVotersResponse {
            non_voters: Vec::new(),
            count: 0,
        });
    };

    let (page_size, _page_number, offset) =
        legacy_page_params(query.page_size.as_deref(), query.page.as_deref());

    let rows = match db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                pnv.id,
                pnv.proposal_id::bigint AS proposal_id,
                pnv.registered_voter_id,
                rv.current_voting_power::text AS current_voting_power
            FROM near.hos_proposal_non_voters pnv
            LEFT JOIN near.hos_registered_voters rv
                ON rv.registered_voter_id = pnv.registered_voter_id
            WHERE pnv.proposal_id = $1
            ORDER BY rv.current_voting_power DESC NULLS LAST, pnv.registered_voter_id ASC
            LIMIT $2 OFFSET $3
            ",
            vec![proposal_id.into(), page_size.into(), offset.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            error!(error = %err, "failed to query Agora proposal non-voters");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal non voters",
            );
        }
    };

    let count = match scalar_i64_with_values(
        db.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.hos_proposal_non_voters WHERE proposal_id = $1",
        vec![proposal_id.into()],
    )
    .await
    {
        Ok(count) => count,
        Err(err) => {
            error!(error = %err, "failed to count Agora proposal non-voters");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal non voters",
            );
        }
    };

    let non_voters = match rows
        .iter()
        .map(to_agora_non_voter_dto)
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(non_voters) => non_voters,
        Err(err) => {
            error!(error = %err, "failed to decode Agora proposal non-voters");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal non voters",
            );
        }
    };

    json_ok(AgoraProposalNonVotersResponse { non_voters, count })
}

#[utoipa::path(
    get,
    path = "/api/agora/proposal/{proposal_id}/charts",
    summary = "Get proposal chart data",
    description = "Returns chart rows from the latest proposal voting history using the legacy Agora payload shape.",
    params(("proposal_id" = String, Path, description = "Proposal id")),
    tag = "agora",
    operation_id = "getAgoraProposalCharts",
    responses(
        (status = 200, description = "Proposal chart rows", body = AgoraProposalChartsResponse),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_proposal_charts(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<String>,
) -> Response {
    let db = match require_db(&app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    let Some(proposal_id) = parse_js_int(proposal_id.as_str()) else {
        return json_ok(AgoraProposalChartsResponse { data: Vec::new() });
    };

    let rows = match db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                voter_id,
                vote_option::text AS vote_option,
                voting_power::text AS voting_power,
                voted_at
            FROM near.hos_proposal_voting_history
            WHERE proposal_id = $1
            ORDER BY voted_at ASC
            ",
            vec![proposal_id.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            error!(error = %err, "failed to query Agora proposal charts");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal voting charts data",
            );
        }
    };

    let data = match rows
        .iter()
        .map(to_agora_chart_point_dto)
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(data) => data,
        Err(err) => {
            error!(error = %err, "failed to decode Agora proposal charts");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal voting charts data",
            );
        }
    };

    json_ok(AgoraProposalChartsResponse { data })
}

#[utoipa::path(
    get,
    path = "/api/agora/proposal/{proposal_id}/quorum",
    summary = "Get proposal quorum",
    description = "Returns the calculated quorum amount for one approved proposal in the legacy Agora payload shape.",
    params(("proposal_id" = String, Path, description = "Proposal id")),
    tag = "agora",
    operation_id = "getAgoraProposalQuorum",
    responses(
        (status = 200, description = "Proposal quorum", body = AgoraProposalQuorumResponse),
        (status = 400, description = "Proposal is not approved", body = AgoraLegacyErrorBody),
        (status = 404, description = "Proposal not found", body = AgoraLegacyErrorBody),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_proposal_quorum(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<String>,
) -> Response {
    let db = match require_db(&app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    let row = match db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                proposal_description,
                total_venear_at_approval::text AS total_venear_at_approval,
                is_approved
            FROM near.hos_proposals
            WHERE proposal_id::text = $1
            LIMIT 1
            ",
            vec![proposal_id.clone().into()],
        ))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return json_error(StatusCode::NOT_FOUND, "Proposal not found"),
        Err(err) => {
            error!(error = %err, "failed to query Agora proposal quorum");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal quorum",
            );
        }
    };

    let is_approved = match decode_row_col::<bool>(&row, "is_approved", "Agora proposal quorum") {
        Ok(is_approved) => is_approved,
        Err(err) => {
            error!(error = %err, "failed to decode Agora proposal quorum");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal quorum",
            );
        }
    };

    if !is_approved {
        return json_error(StatusCode::BAD_REQUEST, "Proposal is not approved");
    }

    let description = match decode_row_col::<Option<String>>(
        &row,
        "proposal_description",
        "Agora proposal quorum",
    ) {
        Ok(value) => value.unwrap_or_default(),
        Err(err) => {
            error!(error = %err, "failed to decode Agora proposal description");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal quorum",
            );
        }
    };

    let metadata = match decode_metadata(description.as_str()) {
        Ok(metadata) => metadata,
        Err(err) => {
            error!(error = %err, "failed to decode Agora proposal metadata");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal quorum",
            );
        }
    };

    let total_voting_power = match decode_row_col::<Option<String>>(
        &row,
        "total_venear_at_approval",
        "Agora proposal quorum",
    ) {
        Ok(value) => value,
        Err(err) => {
            error!(error = %err, "failed to decode Agora total voting power");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch proposal quorum",
            );
        }
    };

    json_ok(AgoraProposalQuorumResponse {
        quorum_amount: calculate_quorum_amount(total_voting_power.as_deref(), &metadata),
    })
}

#[utoipa::path(
    get,
    path = "/api/agora/delegates",
    summary = "List delegates",
    description = "Returns delegates using indexed voter data in the legacy Agora payload shape.",
    params(AgoraDelegatesQuery),
    tag = "agora",
    operation_id = "getAgoraDelegates",
    responses(
        (status = 200, description = "Delegate rows", body = AgoraDelegatesResponse),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_delegates(
    State(app_state): State<AppState>,
    Query(query): Query<AgoraDelegatesQuery>,
) -> Response {
    let db = match require_db(&app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    if delegates_filter_requires_web2(&query) {
        return json_ok(AgoraDelegatesResponse {
            delegates: Vec::new(),
            count: 0,
        });
    }

    let (page_size, _page_number, offset) =
        legacy_page_params(query.page_size.as_deref(), query.page.as_deref());
    let seed = parse_seed(query.sorting_seed.as_deref());
    let order_clause = delegates_order_clause(query.order_by.as_deref());

    let txn = match db.begin().await {
        Ok(txn) => txn,
        Err(err) => {
            error!(error = %err, "failed to start delegate compatibility transaction");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegates",
            );
        }
    };

    if order_clause.contains("random()") {
        if let Err(err) = txn
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT setseed($1)",
                vec![seed.into()],
            ))
            .await
        {
            error!(error = %err, "failed to apply delegate ordering seed");
            let _ = txn.rollback().await;
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegates",
            );
        }
    }

    let query_sql = format!(
        "
        SELECT
            registered_voter_id AS address,
            current_voting_power::text AS voting_power,
            proposal_participation_rate::text AS participation_rate
        FROM near.hos_registered_voters
        ORDER BY {order_clause}
        LIMIT $1 OFFSET $2
        "
    );

    let rows = match txn
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            query_sql,
            vec![page_size.into(), offset.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            error!(error = %err, "failed to query Agora delegates");
            let _ = txn.rollback().await;
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegates",
            );
        }
    };

    let _ = txn.rollback().await;

    let count = match scalar_i64_with_values(
        db.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.hos_registered_voters",
        vec![],
    )
    .await
    {
        Ok(count) => count,
        Err(err) => {
            error!(error = %err, "failed to count Agora delegates");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegates",
            );
        }
    };

    let delegates = match rows
        .iter()
        .map(to_agora_delegate_summary_dto)
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(delegates) => delegates,
        Err(err) => {
            error!(error = %err, "failed to decode Agora delegates");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegates",
            );
        }
    };

    json_ok(AgoraDelegatesResponse { delegates, count })
}

#[utoipa::path(
    get,
    path = "/api/agora/delegates/{address}",
    summary = "Get one delegate",
    description = "Returns one delegate using indexed voter data in the legacy Agora payload shape.",
    params(("address" = String, Path, description = "Delegate address")),
    tag = "agora",
    operation_id = "getAgoraDelegate",
    responses(
        (status = 200, description = "Delegate detail", body = AgoraDelegateDetailResponse),
        (status = 404, description = "Delegate not found", body = AgoraLegacyMessageBody),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_delegate(
    State(app_state): State<AppState>,
    Path(address): Path<String>,
) -> Response {
    let db = match require_db(&app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    let row = match db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                registered_voter_id AS address,
                current_voting_power::text AS voting_power,
                proposal_participation_rate::text AS participation_rate
            FROM near.hos_registered_voters
            WHERE registered_voter_id = $1
            LIMIT 1
            ",
            vec![address.clone().into()],
        ))
        .await
    {
        Ok(Some(row)) => row,
        Ok(None) => return json_message(StatusCode::NOT_FOUND, DELEGATE_NOT_FOUND_MESSAGE),
        Err(err) => {
            error!(error = %err, "failed to query Agora delegate");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegate",
            );
        }
    };

    let counts = match fetch_delegate_detail_counts(db.as_ref(), address.as_str()).await {
        Ok(counts) => counts,
        Err(err) => {
            error!(error = %err, "failed to query Agora delegate counters");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegate",
            );
        }
    };

    let delegate = match to_agora_delegate_detail_dto(&row, counts) {
        Ok(delegate) => delegate,
        Err(err) => {
            error!(error = %err, "failed to decode Agora delegate detail");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegate",
            );
        }
    };

    json_ok(AgoraDelegateDetailResponse { delegate })
}

#[utoipa::path(
    get,
    path = "/api/agora/delegates/{address}/voting-history",
    summary = "List delegate voting history",
    description = "Returns latest proposal-vote rows for one delegate in the legacy Agora payload shape.",
    params(
        ("address" = String, Path, description = "Delegate address"),
        AgoraPaginationQuery
    ),
    tag = "agora",
    operation_id = "getAgoraDelegateVotingHistory",
    responses(
        (status = 200, description = "Delegate vote rows", body = AgoraDelegateVotingHistoryResponse),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_delegate_voting_history(
    State(app_state): State<AppState>,
    Path(address): Path<String>,
    Query(query): Query<AgoraPaginationQuery>,
) -> Response {
    let db = match require_db(&app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    let (page_size, _page_number, offset) =
        legacy_page_params(query.page_size.as_deref(), query.page.as_deref());

    let rows = match db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                h.voter_id AS address,
                h.vote_option::text AS vote_option,
                h.voting_power::text AS voting_power,
                h.voted_at,
                h.proposal_id::text AS proposal_id,
                COALESCE(h.proposal_name, p.proposal_title) AS proposal_name
            FROM near.hos_proposal_voting_history h
            LEFT JOIN near.hos_proposals p
              ON p.proposal_id = h.proposal_id
            WHERE h.voter_id = $1
            ORDER BY h.voted_date DESC
            LIMIT $2 OFFSET $3
            ",
            vec![address.clone().into(), page_size.into(), offset.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            error!(error = %err, "failed to query Agora delegate voting history");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegate voting history",
            );
        }
    };

    let count = match scalar_i64_with_values(
        db.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM near.hos_proposal_voting_history WHERE voter_id = $1",
        vec![address.into()],
    )
    .await
    {
        Ok(count) => count,
        Err(err) => {
            error!(error = %err, "failed to count Agora delegate voting history");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegate voting history",
            );
        }
    };

    let votes = match rows
        .iter()
        .map(to_agora_delegate_voting_history_dto)
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(votes) => votes,
        Err(err) => {
            error!(error = %err, "failed to decode Agora delegate voting history");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegate voting history",
            );
        }
    };

    json_ok(AgoraDelegateVotingHistoryResponse { votes, count })
}

#[utoipa::path(
    get,
    path = "/api/agora/delegates/{address}/delegated-from",
    summary = "List inbound delegate delegations",
    description = "Returns latest inbound delegation rows for a delegate in the legacy Agora payload shape.",
    params(
        ("address" = String, Path, description = "Delegate address"),
        AgoraPaginationQuery
    ),
    tag = "agora",
    operation_id = "getAgoraDelegateDelegatedFrom",
    responses(
        (status = 200, description = "Inbound delegation rows", body = AgoraDelegationEventsResponse),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_delegate_delegated_from(
    State(app_state): State<AppState>,
    Path(address): Path<String>,
    Query(query): Query<AgoraPaginationQuery>,
) -> Response {
    get_delegation_events_response(
        &app_state,
        &address,
        query.page_size.as_deref(),
        query.page.as_deref(),
        DelegationDirection::Inbound,
    )
    .await
}

#[utoipa::path(
    get,
    path = "/api/agora/delegates/{address}/delegated-to",
    summary = "List outbound delegate delegations",
    description = "Returns latest outbound delegation rows for a delegate in the legacy Agora payload shape.",
    params(
        ("address" = String, Path, description = "Delegate address"),
        AgoraPaginationQuery
    ),
    tag = "agora",
    operation_id = "getAgoraDelegateDelegatedTo",
    responses(
        (status = 200, description = "Outbound delegation rows", body = AgoraDelegationEventsResponse),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_delegate_delegated_to(
    State(app_state): State<AppState>,
    Path(address): Path<String>,
    Query(query): Query<AgoraPaginationQuery>,
) -> Response {
    get_delegation_events_response(
        &app_state,
        &address,
        query.page_size.as_deref(),
        query.page.as_deref(),
        DelegationDirection::Outbound,
    )
    .await
}

#[utoipa::path(
    get,
    path = "/api/agora/delegates/{address}/hos-activity",
    summary = "List delegate HOS activity",
    description = "Returns raw indexed HOS activity rows in the legacy Agora payload shape without live RPC augmentation.",
    params(
        ("address" = String, Path, description = "Delegate address"),
        AgoraDelegateHosActivityQuery
    ),
    tag = "agora",
    operation_id = "getAgoraDelegateHosActivity",
    responses(
        (status = 200, description = "Delegate HOS activity rows", body = AgoraHosActivityResponse),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_delegate_hos_activity(
    State(app_state): State<AppState>,
    Path(address): Path<String>,
    Query(query): Query<AgoraDelegateHosActivityQuery>,
) -> Response {
    let _legacy_params = (query.network_id.as_deref(), query.contract_id.as_deref());
    let db = match require_db(&app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    let (page_size, _page_number, offset) =
        legacy_page_params(query.page_size.as_deref(), query.page.as_deref());

    let rows = match db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                receipt_id,
                block_height::text AS block_height,
                event_date,
                near_amount::text AS near_amount,
                locked_near_balance::text AS locked_near_balance,
                method_name,
                event_type
            FROM near.hos_user_activities
            WHERE account_id = $1
              AND (
                    (method_name = 'on_lockup_update' AND event_type = 'on_lockup_update_ft_burn')
                 OR (method_name = 'on_lockup_update' AND event_type = 'on_lockup_update_ft_mint')
                 OR (method_name = 'delegate_all' AND event_type = 'delegate_all_ft_burn')
                 OR (method_name = 'on_lockup_deployed' AND event_type = 'lockup_deployed')
                 OR (method_name = 'delegate_all' AND event_type = 'delegate_all_ft_mint')
                 OR method_name = 'withdraw_from_staking_pool'
                 OR method_name = 'withdraw_all_from_staking_pool'
                 OR method_name = 'unstake'
                 OR method_name = 'unstake_all'
              )
            ORDER BY event_timestamp DESC
            LIMIT $2 OFFSET $3
            ",
            vec![address.clone().into(), page_size.into(), offset.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            error!(error = %err, "failed to query Agora HOS activity");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegate HOS activity",
            );
        }
    };

    let count = match scalar_i64_with_values(
        db.as_ref(),
        "
        SELECT COUNT(*)::bigint AS value
        FROM near.hos_user_activities
        WHERE account_id = $1
          AND (
                (method_name = 'on_lockup_update' AND event_type = 'on_lockup_update_ft_burn')
             OR (method_name = 'on_lockup_update' AND event_type = 'on_lockup_update_ft_mint')
             OR (method_name = 'delegate_all' AND event_type = 'delegate_all_ft_burn')
             OR (method_name = 'on_lockup_deployed' AND event_type = 'lockup_deployed')
             OR (method_name = 'delegate_all' AND event_type = 'delegate_all_ft_mint')
             OR method_name = 'withdraw_from_staking_pool'
             OR method_name = 'withdraw_all_from_staking_pool'
             OR method_name = 'unstake'
             OR method_name = 'unstake_all'
          )
        ",
        vec![address.into()],
    )
    .await
    {
        Ok(count) => count,
        Err(err) => {
            error!(error = %err, "failed to count Agora HOS activity");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegate HOS activity",
            );
        }
    };

    let hos_activity = match rows
        .iter()
        .map(to_agora_hos_activity_dto)
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(hos_activity) => hos_activity,
        Err(err) => {
            error!(error = %err, "failed to decode Agora HOS activity");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch delegate HOS activity",
            );
        }
    };

    json_ok(AgoraHosActivityResponse {
        hos_activity,
        count,
    })
}

#[utoipa::path(
    get,
    path = "/api/agora/delegate_statement_changes",
    summary = "List delegate statement changes",
    description = "Returns the legacy Agora delegate statement changes shape with an empty on-chain-only payload.",
    params(AgoraDelegateStatementChangesQuery),
    tag = "agora",
    operation_id = "getAgoraDelegateStatementChanges",
    responses(
        (status = 200, description = "Delegate statement change rows", body = AgoraDelegateStatementChangesResponse),
        (status = 400, description = "Invalid pagination", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_delegate_statement_changes(
    Query(query): Query<AgoraDelegateStatementChangesQuery>,
) -> Response {
    let _lookback_days = query.lookback_days.as_deref();
    let offset = match parse_positive_legacy_param(query.offset.as_deref(), DEFAULT_PAGE_SIZE) {
        Ok(offset) => offset,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };
    let page = match parse_positive_legacy_param(query.page.as_deref(), DEFAULT_PAGE_NUMBER) {
        Ok(page) => page,
        Err(message) => return json_error(StatusCode::BAD_REQUEST, message),
    };

    json_ok(AgoraDelegateStatementChangesResponse {
        page,
        offset,
        changes: Vec::new(),
        count: 0,
    })
}

#[utoipa::path(
    get,
    path = "/api/agora/get_voting_power_chart/{account_id}",
    summary = "Get delegate voting power chart",
    description = "Returns the legacy Agora voting-power chart based on indexed lockup and delegation deltas.",
    params(("account_id" = String, Path, description = "Account id")),
    tag = "agora",
    operation_id = "getAgoraVotingPowerChart",
    responses(
        (status = 200, description = "Voting power chart points", body = [AgoraVotingPowerChartPoint]),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_voting_power_chart(
    State(app_state): State<AppState>,
    Path(account_id): Path<String>,
) -> Response {
    let db = match require_db(&app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    let rows = match db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            WITH personal_locks AS (
                SELECT
                    block_height,
                    event_timestamp AS timestamp,
                    locked_near_balance
                    - COALESCE(
                        LAG(locked_near_balance) OVER (
                            PARTITION BY account_id
                            ORDER BY event_timestamp ASC, receipt_id ASC
                        ),
                        0
                    ) AS vp_delta
                FROM near.hos_user_activities
                WHERE account_id = $1
                  AND locked_near_balance IS NOT NULL
            ),
            delegation_changes AS (
                SELECT
                    delegator_account_id,
                    delegatee_account_id,
                    near_amount,
                    block_height,
                    event_timestamp AS timestamp,
                    LAG(delegatee_account_id) OVER (
                        PARTITION BY delegator_account_id
                        ORDER BY event_timestamp ASC, receipt_id ASC
                    ) AS prev_delegatee,
                    LAG(near_amount) OVER (
                        PARTITION BY delegator_account_id
                        ORDER BY event_timestamp ASC, receipt_id ASC
                    ) AS prev_amount
                FROM near.hos_delegation_events
                WHERE near_amount IS NOT NULL
            ),
            inbound_delegations AS (
                SELECT
                    block_height,
                    timestamp,
                    near_amount AS vp_delta
                FROM delegation_changes
                WHERE delegatee_account_id = $1
            ),
            lost_delegations AS (
                SELECT
                    block_height,
                    timestamp,
                    -prev_amount AS vp_delta
                FROM delegation_changes
                WHERE prev_delegatee = $1
                  AND prev_amount IS NOT NULL
            ),
            combined_events AS (
                SELECT * FROM personal_locks
                UNION ALL
                SELECT * FROM inbound_delegations
                UNION ALL
                SELECT * FROM lost_delegations
            ),
            cumulative_vp AS (
                SELECT
                    block_height,
                    timestamp,
                    SUM(vp_delta) OVER (
                        ORDER BY timestamp ASC, block_height ASC
                        ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW
                    ) AS voting_power
                FROM combined_events
            )
            SELECT
                block_height::bigint AS block_height,
                voting_power::text AS voting_power,
                timestamp
            FROM cumulative_vp
            ORDER BY timestamp ASC, block_height ASC
            ",
            vec![account_id.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            error!(error = %err, "failed to query Agora voting power chart");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch voting power chart",
            );
        }
    };

    let data = match rows
        .iter()
        .map(to_agora_voting_power_chart_point)
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(data) => data,
        Err(err) => {
            error!(error = %err, "failed to decode Agora voting power chart");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch voting power chart",
            );
        }
    };

    json_ok(data)
}

#[utoipa::path(
    get,
    path = "/api/agora/vote_changes/{proposal_id}",
    summary = "Get full vote changes",
    description = "Returns grouped multi-vote history for voters who changed their vote on a proposal.",
    params(("proposal_id" = String, Path, description = "Proposal id")),
    tag = "agora",
    operation_id = "getAgoraVoteChanges",
    responses(
        (status = 200, description = "Vote change groups", body = AgoraVoteChangesResponse),
        (status = 500, description = "Compatibility query failed", body = AgoraLegacyErrorBody),
        (status = 503, description = "Database unavailable", body = AgoraLegacyErrorBody)
    )
)]
pub async fn get_vote_changes(
    State(app_state): State<AppState>,
    Path(proposal_id): Path<String>,
) -> Response {
    let db = match require_db(&app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    let Some(proposal_id) = parse_js_int(proposal_id.as_str()) else {
        return json_ok(AgoraVoteChangesResponse {
            vote_changes: Vec::new(),
        });
    };

    let rows = match db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            WITH all_votes AS (
                SELECT
                    ra.predecessor_id AS voter_id,
                    CASE
                        WHEN COALESCE(ra.args_json ->> 'vote', '') ~ '^[0-9]+$'
                            THEN (ra.args_json ->> 'vote')::integer
                        ELSE NULL
                    END AS vote_option,
                    COALESCE(
                        NULLIF(vote_event.event_log -> 'data' -> 0 ->> 'account_balance', ''),
                        '0'
                    ) AS voting_power,
                    TO_TIMESTAMP(ra.block_timestamp::double precision / 1e9) AT TIME ZONE 'UTC' AS voted_at,
                    ra.block_height::bigint AS block_height
                FROM near.near_receipt_actions ra
                INNER JOIN near.near_execution_outcomes eo
                    ON eo.receipt_id = ra.receipt_id
                   AND eo.status = 'SuccessValue'
                LEFT JOIN LATERAL (
                    SELECT near.safe_json_parse(REPLACE(log_line, 'EVENT_JSON:', '')) AS event_log
                    FROM jsonb_array_elements_text(COALESCE(eo.logs, '[]'::jsonb)) logs(log_line)
                    WHERE COALESCE(log_line, '') LIKE 'EVENT_JSON:%'
                      AND COALESCE(
                            near.safe_json_parse(REPLACE(log_line, 'EVENT_JSON:', '')) ->> 'error',
                            ''
                          ) = ''
                      AND near.safe_json_parse(REPLACE(log_line, 'EVENT_JSON:', '')) ->> 'event' IN ('add_vote', 'remove_vote')
                    LIMIT 1
                ) vote_event ON TRUE
                WHERE ra.method_name = 'vote'
                  AND ra.receiver_id = (
                        SELECT hos_contract_address
                        FROM near.hos_proposals
                        WHERE proposal_id = $1
                        LIMIT 1
                  )
                  AND near.is_hos_action(ra.receiver_id, ra.predecessor_id, ra.signer_account_id) = TRUE
                  AND CASE
                        WHEN COALESCE(ra.args_json ->> 'proposal_id', '') ~ '^[0-9]+$'
                            THEN (ra.args_json ->> 'proposal_id')::numeric
                        ELSE NULL
                      END = $1
            ),
            voters_with_changes AS (
                SELECT voter_id
                FROM all_votes
                GROUP BY voter_id
                HAVING COUNT(*) > 1
            )
            SELECT
                av.voter_id,
                av.vote_option,
                av.voting_power,
                av.voted_at,
                av.block_height
            FROM all_votes av
            INNER JOIN voters_with_changes vc ON vc.voter_id = av.voter_id
            ORDER BY av.voter_id ASC, av.voted_at ASC, av.block_height ASC
            ",
            vec![proposal_id.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            error!(error = %err, "failed to query Agora vote changes");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch vote changes",
            );
        }
    };

    let vote_changes = match group_vote_changes(&rows) {
        Ok(vote_changes) => vote_changes,
        Err(err) => {
            error!(error = %err, "failed to decode Agora vote changes");
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to fetch vote changes",
            );
        }
    };

    json_ok(AgoraVoteChangesResponse { vote_changes })
}

enum DelegationDirection {
    Inbound,
    Outbound,
}

async fn get_delegation_events_response(
    app_state: &AppState,
    address: &str,
    page_size: Option<&str>,
    page: Option<&str>,
    direction: DelegationDirection,
) -> Response {
    let db = match require_db(app_state) {
        Ok(db) => db,
        Err(response) => return response,
    };

    let (page_size, _page_number, offset) = legacy_page_params(page_size, page);
    let (sql, count_sql, label) = match direction {
        DelegationDirection::Inbound => (
            "
            SELECT
                id,
                receipt_id,
                event_date,
                event_timestamp,
                hos_contract_address,
                delegator_account_id,
                delegatee_account_id,
                delegate_method,
                delegate_event,
                is_latest_delegator_event,
                owner_id,
                near_amount::text AS near_amount,
                block_height::text AS block_height,
                block_hash
            FROM near.hos_delegation_events
            WHERE delegatee_account_id = $1
              AND is_latest_delegator_event = TRUE
              AND delegate_event = 'ft_mint'
            ORDER BY event_timestamp DESC
            LIMIT $2 OFFSET $3
            ",
            "
            SELECT COUNT(*)::bigint AS value
            FROM near.hos_delegation_events
            WHERE delegatee_account_id = $1
              AND is_latest_delegator_event = TRUE
              AND delegate_event = 'ft_mint'
            ",
            "Failed to fetch delegate delegated from events",
        ),
        DelegationDirection::Outbound => (
            "
            SELECT
                id,
                receipt_id,
                event_date,
                event_timestamp,
                hos_contract_address,
                delegator_account_id,
                delegatee_account_id,
                delegate_method,
                delegate_event,
                is_latest_delegator_event,
                owner_id,
                near_amount::text AS near_amount,
                block_height::text AS block_height,
                block_hash
            FROM near.hos_delegation_events
            WHERE delegator_account_id = $1
              AND is_latest_delegator_event = TRUE
              AND delegate_method = 'delegate_all'
              AND delegate_event = 'ft_mint'
            ORDER BY event_timestamp DESC
            LIMIT $2 OFFSET $3
            ",
            "
            SELECT COUNT(*)::bigint AS value
            FROM near.hos_delegation_events
            WHERE delegator_account_id = $1
              AND is_latest_delegator_event = TRUE
              AND delegate_method = 'delegate_all'
              AND delegate_event = 'ft_mint'
            ",
            "Failed to fetch delegate delegated to events",
        ),
    };

    let rows = match db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![address.into(), page_size.into(), offset.into()],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            error!(error = %err, "failed to query Agora delegation events");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, label);
        }
    };

    let count = match scalar_i64_with_values(db.as_ref(), count_sql, vec![address.into()]).await {
        Ok(count) => count,
        Err(err) => {
            error!(error = %err, "failed to count Agora delegation events");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, label);
        }
    };

    let events = match rows
        .iter()
        .map(to_agora_delegation_event_dto)
        .collect::<Result<Vec<_>, String>>()
    {
        Ok(events) => events,
        Err(err) => {
            error!(error = %err, "failed to decode Agora delegation events");
            return json_error(StatusCode::INTERNAL_SERVER_ERROR, label);
        }
    };

    json_ok(AgoraDelegationEventsResponse { events, count })
}

async fn fetch_legacy_proposal_list(
    db: &DatabaseConnection,
    approved: Option<bool>,
    created_by: Option<String>,
    page_size: i64,
    offset: i64,
) -> Result<AgoraProposalListResponse, String> {
    let (approval_clause, rejected_clause) = match approved {
        Some(true) => ("is_approved = TRUE", "AND is_rejected = FALSE"),
        Some(false) => ("is_approved = FALSE", "AND is_rejected = FALSE"),
        None => ("TRUE", ""),
    };

    let list_sql = format!(
        "
        SELECT
            receipt_id,
            proposal_id::text AS proposal_id,
            proposal_title,
            proposal_description,
            proposal_url,
            is_approved,
            is_rejected,
            has_votes,
            created_at,
            creator_id,
            approved_at,
            voting_start_at,
            approver_id,
            rejected_at,
            rejecter_id,
            voting_duration_ns::text AS voting_duration_ns,
            total_venear_at_approval::text AS total_venear_at_approval,
            for_voting_power::text AS for_voting_power,
            against_voting_power::text AS against_voting_power,
            abstain_voting_power::text AS abstain_voting_power
        FROM near.hos_proposals
        WHERE {approval_clause}
          {rejected_clause}
          AND ($1::text IS NULL OR creator_id = $1)
        ORDER BY created_at DESC NULLS LAST
        LIMIT $2 OFFSET $3
        "
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            list_sql,
            vec![created_by.clone().into(), page_size.into(), offset.into()],
        ))
        .await
        .map_err(|err| err.to_string())?;

    let count_sql = format!(
        "
        SELECT COUNT(*)::bigint AS value
        FROM near.hos_proposals
        WHERE {approval_clause}
          {rejected_clause}
          AND ($1::text IS NULL OR creator_id = $1)
        "
    );

    let count = scalar_i64_with_values(db, count_sql.as_str(), vec![created_by.into()]).await?;

    let proposals = rows
        .iter()
        .map(to_agora_proposal_dto)
        .collect::<Result<Vec<_>, String>>()?;

    Ok(AgoraProposalListResponse { proposals, count })
}

async fn fetch_delegate_detail_counts(
    db: &DatabaseConnection,
    address: &str,
) -> Result<(i64, i64, i64, i64), String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "
            SELECT
                COALESCE(SUM(CASE WHEN vote_option = 0 THEN 1 ELSE 0 END), 0)::bigint AS for_count,
                COALESCE(SUM(CASE WHEN vote_option = 1 THEN 1 ELSE 0 END), 0)::bigint AS against_count,
                COALESCE(SUM(CASE WHEN vote_option = 2 THEN 1 ELSE 0 END), 0)::bigint AS abstain_count,
                (
                    SELECT COUNT(*)::bigint
                    FROM near.hos_delegation_events
                    WHERE delegatee_account_id = $1
                      AND is_latest_delegator_event = TRUE
                      AND delegate_method = 'delegate_all'
                      AND delegate_event = 'ft_mint'
                ) AS delegated_from_count
            FROM near.hos_proposal_voting_history
            WHERE voter_id = $1
            ",
            vec![address.into()],
        ))
        .await
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "missing delegate counts row".to_string())?;

    Ok((
        decode_row_col::<i64>(&row, "for_count", "Agora delegate counts")?,
        decode_row_col::<i64>(&row, "against_count", "Agora delegate counts")?,
        decode_row_col::<i64>(&row, "abstain_count", "Agora delegate counts")?,
        decode_row_col::<i64>(&row, "delegated_from_count", "Agora delegate counts")?,
    ))
}

fn to_agora_proposal_dto(row: &QueryResult) -> Result<AgoraProposalDto, String> {
    let receipt_id = decode_row_col::<String>(row, "receipt_id", "Agora proposal")?;
    let proposal_description =
        decode_row_col::<Option<String>>(row, "proposal_description", "Agora proposal")?;
    let metadata = decode_metadata(proposal_description.as_deref().unwrap_or_default())?;
    let created_at = decode_row_col::<Option<NaiveDateTime>>(row, "created_at", "Agora proposal")?;
    let voting_start_at =
        decode_row_col::<Option<NaiveDateTime>>(row, "voting_start_at", "Agora proposal")?;
    let approved_at =
        decode_row_col::<Option<NaiveDateTime>>(row, "approved_at", "Agora proposal")?;
    let rejected_at =
        decode_row_col::<Option<NaiveDateTime>>(row, "rejected_at", "Agora proposal")?;
    let voting_duration_ns =
        decode_row_col::<Option<String>>(row, "voting_duration_ns", "Agora proposal")?;
    let total_voting_power =
        decode_row_col::<Option<String>>(row, "total_venear_at_approval", "Agora proposal")?;
    let is_approved = decode_row_col::<bool>(row, "is_approved", "Agora proposal")?;
    let is_rejected = decode_row_col::<bool>(row, "is_rejected", "Agora proposal")?;
    let status = get_derived_proposal_status(
        is_approved,
        is_rejected,
        voting_start_at,
        voting_duration_ns.as_deref(),
    );

    Ok(AgoraProposalDto {
        id: receipt_id.clone(),
        proposal_type: metadata.proposal_type.as_str().to_string(),
        approval_threshold: metadata.approval_threshold.to_string(),
        approved_at: approved_at.map(format_naive_datetime),
        approver_id: decode_row_col::<Option<String>>(row, "approver_id", "Agora proposal")?,
        created_at: created_at.map(format_naive_datetime),
        creator_id: decode_row_col::<Option<String>>(row, "creator_id", "Agora proposal")?,
        has_vote: decode_row_col::<bool>(row, "has_votes", "Agora proposal")?,
        is_approved,
        is_rejected,
        proposal_description,
        proposal_id: decode_row_col::<Option<String>>(row, "proposal_id", "Agora proposal")?,
        proposal_title: decode_row_col::<Option<String>>(row, "proposal_title", "Agora proposal")?,
        proposal_url: decode_row_col::<Option<String>>(row, "proposal_url", "Agora proposal")?,
        receipt_id,
        rejected_at: rejected_at.map(format_naive_datetime),
        rejecter_id: decode_row_col::<Option<String>>(row, "rejecter_id", "Agora proposal")?,
        for_voting_power: normalize_numeric_string_opt(decode_row_col::<Option<String>>(
            row,
            "for_voting_power",
            "Agora proposal",
        )?),
        against_voting_power: normalize_numeric_string_opt(decode_row_col::<Option<String>>(
            row,
            "against_voting_power",
            "Agora proposal",
        )?),
        abstain_voting_power: normalize_numeric_string_opt(decode_row_col::<Option<String>>(
            row,
            "abstain_voting_power",
            "Agora proposal",
        )?),
        voting_duration_ns: is_approved
            .then(|| normalize_numeric_string_opt(voting_duration_ns))
            .flatten(),
        total_voting_power: is_approved
            .then(|| normalize_numeric_string_opt(total_voting_power.clone()))
            .flatten(),
        quorum_amount: calculate_quorum_amount(total_voting_power.as_deref(), &metadata),
        status,
        voting_start_time_ns: voting_start_at.map(naive_datetime_to_ns_string),
        voting_created_at_ns: created_at.map(naive_datetime_to_ns_string),
    })
}

fn to_agora_proposal_vote_dto(row: &QueryResult) -> Result<AgoraProposalVoteDto, String> {
    Ok(AgoraProposalVoteDto {
        account_id: decode_row_col::<String>(row, "voter_id", "Agora proposal vote")?,
        voting_power: normalize_numeric_string(
            decode_row_col::<Option<String>>(row, "voting_power_text", "Agora proposal vote")?
                .as_deref()
                .unwrap_or("0"),
        ),
        vote_option: decode_row_col::<Option<String>>(row, "vote_option", "Agora proposal vote")?
            .unwrap_or_else(|| "0".to_string()),
    })
}

fn to_agora_chart_point_dto(row: &QueryResult) -> Result<AgoraProposalChartPointDto, String> {
    Ok(AgoraProposalChartPointDto {
        account_id: decode_row_col::<String>(row, "voter_id", "Agora proposal chart")?,
        voting_power: normalize_numeric_string(
            decode_row_col::<Option<String>>(row, "voting_power", "Agora proposal chart")?
                .as_deref()
                .unwrap_or("0"),
        ),
        vote_option: decode_row_col::<Option<String>>(row, "vote_option", "Agora proposal chart")?
            .unwrap_or_else(|| "0".to_string()),
        voted_at: decode_row_col::<Option<NaiveDateTime>>(row, "voted_at", "Agora proposal chart")?
            .map(format_naive_datetime),
    })
}

fn to_agora_non_voter_dto(row: &QueryResult) -> Result<AgoraProposalNonVoterDto, String> {
    Ok(AgoraProposalNonVoterDto {
        id: decode_row_col::<String>(row, "id", "Agora proposal non-voter")?,
        proposal_id: decode_row_col::<i64>(row, "proposal_id", "Agora proposal non-voter")?
            .to_string(),
        registered_voter_id: decode_row_col::<String>(
            row,
            "registered_voter_id",
            "Agora proposal non-voter",
        )?,
        voting_power: normalize_numeric_string(
            decode_row_col::<Option<String>>(
                row,
                "current_voting_power",
                "Agora proposal non-voter",
            )?
            .as_deref()
            .unwrap_or("0"),
        ),
    })
}

fn to_agora_delegate_summary_dto(row: &QueryResult) -> Result<AgoraDelegateSummaryDto, String> {
    Ok(AgoraDelegateSummaryDto {
        address: decode_row_col::<String>(row, "address", "Agora delegate")?,
        voting_power: normalize_numeric_string_opt(decode_row_col::<Option<String>>(
            row,
            "voting_power",
            "Agora delegate",
        )?),
        participation_rate: normalize_numeric_string_opt(decode_row_col::<Option<String>>(
            row,
            "participation_rate",
            "Agora delegate",
        )?),
        twitter: None,
        discord: None,
        warpcast: None,
        statement: None,
        top_issues: None,
        endorsed: None,
        notification_preferences: None,
    })
}

fn to_agora_delegate_detail_dto(
    row: &QueryResult,
    counts: (i64, i64, i64, i64),
) -> Result<AgoraDelegateDetailDto, String> {
    Ok(AgoraDelegateDetailDto {
        address: decode_row_col::<String>(row, "address", "Agora delegate detail")?,
        twitter: None,
        discord: None,
        warpcast: None,
        statement: None,
        top_issues: None,
        endorsed: None,
        notification_preferences: None,
        voting_power: normalize_numeric_string_opt(decode_row_col::<Option<String>>(
            row,
            "voting_power",
            "Agora delegate detail",
        )?),
        for_count: counts.0,
        against_count: counts.1,
        abstain_count: counts.2,
        delegated_from_count: counts.3,
        participation_rate: normalize_numeric_string_opt(decode_row_col::<Option<String>>(
            row,
            "participation_rate",
            "Agora delegate detail",
        )?),
    })
}

fn to_agora_delegate_voting_history_dto(
    row: &QueryResult,
) -> Result<AgoraDelegateVotingHistoryDto, String> {
    Ok(AgoraDelegateVotingHistoryDto {
        vote_option: decode_row_col::<Option<String>>(row, "vote_option", "Agora delegate vote")?,
        voting_power: normalize_numeric_string(
            decode_row_col::<Option<String>>(row, "voting_power", "Agora delegate vote")?
                .as_deref()
                .unwrap_or("0"),
        ),
        address: decode_row_col::<Option<String>>(row, "address", "Agora delegate vote")?,
        voted_at: decode_row_col::<Option<NaiveDateTime>>(row, "voted_at", "Agora delegate vote")?
            .map(format_naive_datetime),
        proposal_id: decode_row_col::<Option<String>>(row, "proposal_id", "Agora delegate vote")?,
        proposal_name: decode_row_col::<Option<String>>(
            row,
            "proposal_name",
            "Agora delegate vote",
        )?,
    })
}

fn to_agora_delegation_event_dto(row: &QueryResult) -> Result<AgoraDelegationEventDto, String> {
    Ok(AgoraDelegationEventDto {
        id: decode_row_col::<String>(row, "id", "Agora delegation event")?,
        receipt_id: decode_row_col::<String>(row, "receipt_id", "Agora delegation event")?,
        event_date: decode_row_col::<Option<NaiveDate>>(
            row,
            "event_date",
            "Agora delegation event",
        )?
        .map(format_naive_date),
        event_timestamp: decode_row_col::<Option<NaiveDateTime>>(
            row,
            "event_timestamp",
            "Agora delegation event",
        )?
        .map(format_naive_datetime),
        hos_contract_address: decode_row_col::<Option<String>>(
            row,
            "hos_contract_address",
            "Agora delegation event",
        )?,
        delegator_id: decode_row_col::<String>(
            row,
            "delegator_account_id",
            "Agora delegation event",
        )?,
        delegatee_id: decode_row_col::<Option<String>>(
            row,
            "delegatee_account_id",
            "Agora delegation event",
        )?,
        delegate_method: decode_row_col::<Option<String>>(
            row,
            "delegate_method",
            "Agora delegation event",
        )?,
        delegate_event: decode_row_col::<Option<String>>(
            row,
            "delegate_event",
            "Agora delegation event",
        )?,
        is_latest_delegator_event: decode_row_col::<bool>(
            row,
            "is_latest_delegator_event",
            "Agora delegation event",
        )?,
        owner_id: decode_row_col::<Option<String>>(row, "owner_id", "Agora delegation event")?,
        near_amount: normalize_numeric_string_opt(decode_row_col::<Option<String>>(
            row,
            "near_amount",
            "Agora delegation event",
        )?),
        block_height: decode_row_col::<Option<String>>(
            row,
            "block_height",
            "Agora delegation event",
        )?,
        block_hash: decode_row_col::<Option<String>>(row, "block_hash", "Agora delegation event")?,
    })
}

fn to_agora_hos_activity_dto(row: &QueryResult) -> Result<AgoraHosActivityDto, String> {
    let method_name = decode_row_col::<Option<String>>(row, "method_name", "Agora HOS activity")?
        .unwrap_or_default();
    let event_type = decode_row_col::<Option<String>>(row, "event_type", "Agora HOS activity")?
        .unwrap_or_default();

    Ok(AgoraHosActivityDto {
        receipt_id: decode_row_col::<String>(row, "receipt_id", "Agora HOS activity")?,
        block_height: decode_row_col::<Option<String>>(row, "block_height", "Agora HOS activity")?,
        event_date: decode_row_col::<Option<NaiveDate>>(row, "event_date", "Agora HOS activity")?
            .map(format_naive_date),
        near_amount: normalize_numeric_string_opt(decode_row_col::<Option<String>>(
            row,
            "near_amount",
            "Agora HOS activity",
        )?),
        locked_near_balance: normalize_numeric_string_opt(decode_row_col::<Option<String>>(
            row,
            "locked_near_balance",
            "Agora HOS activity",
        )?),
        transaction_type: legacy_transaction_type(method_name.as_str(), event_type.as_str())
            .to_string(),
    })
}

fn to_agora_voting_power_chart_point(
    row: &QueryResult,
) -> Result<AgoraVotingPowerChartPoint, String> {
    Ok(AgoraVotingPowerChartPoint {
        block_number: decode_row_col::<i64>(row, "block_height", "Agora voting power chart")?,
        vp: normalize_numeric_string(
            decode_row_col::<String>(row, "voting_power", "Agora voting power chart")?.as_str(),
        ),
        timestamp: format_naive_datetime(decode_row_col::<NaiveDateTime>(
            row,
            "timestamp",
            "Agora voting power chart",
        )?),
    })
}

fn group_vote_changes(rows: &[QueryResult]) -> Result<Vec<AgoraVoteChangeDto>, String> {
    let mut grouped: Vec<AgoraVoteChangeDto> = Vec::new();

    for row in rows {
        let voter_id = decode_row_col::<String>(row, "voter_id", "Agora vote change")?;
        let change = AgoraVoteChangeEntryDto {
            vote_option: decode_row_col::<Option<i32>>(row, "vote_option", "Agora vote change")?
                .unwrap_or_default(),
            voting_power: normalize_numeric_string(
                decode_row_col::<String>(row, "voting_power", "Agora vote change")?.as_str(),
            ),
            voted_at: format_naive_datetime(decode_row_col::<NaiveDateTime>(
                row,
                "voted_at",
                "Agora vote change",
            )?),
            block_height: decode_row_col::<i64>(row, "block_height", "Agora vote change")?,
        };

        if let Some(existing) = grouped
            .last_mut()
            .filter(|entry| entry.account_id == voter_id)
        {
            existing.changes.push(change);
        } else {
            grouped.push(AgoraVoteChangeDto {
                account_id: voter_id,
                changes: vec![change],
            });
        }
    }

    Ok(grouped)
}

fn delegates_filter_requires_web2(query: &AgoraDelegatesQuery) -> bool {
    matches!(
        normalized_text(query.filter_by.as_deref()).as_deref(),
        Some("endorsed")
    ) || query
        .issue_type
        .as_deref()
        .map(str::trim)
        .is_some_and(|value| !value.is_empty())
}

fn delegates_order_clause(order_by: Option<&str>) -> &'static str {
    match normalized_text(order_by)
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("most_voting_power") => "current_voting_power DESC NULLS LAST",
        Some("least_voting_power") => "current_voting_power ASC NULLS FIRST",
        _ => "-LN(random()) / NULLIF(current_voting_power::double precision, 0) NULLS LAST",
    }
}

fn parse_seed(value: Option<&str>) -> f64 {
    value
        .and_then(|raw| raw.trim().parse::<f64>().ok())
        .unwrap_or_else(|| f64::from(Utc::now().timestamp_subsec_nanos()) / 1_000_000_000.0)
}

fn legacy_page_params(page_size: Option<&str>, page: Option<&str>) -> (i64, i64, i64) {
    let page_size = legacy_param_with_default(page_size, DEFAULT_PAGE_SIZE);
    let page = legacy_param_with_default(page, DEFAULT_PAGE_NUMBER);
    let offset = (page - 1).saturating_mul(page_size);
    (page_size, page, offset)
}

fn legacy_param_with_default(value: Option<&str>, default: i64) -> i64 {
    match value.and_then(parse_js_int) {
        Some(0) | None => default,
        Some(value) => value,
    }
}

fn parse_positive_legacy_param(value: Option<&str>, default: i64) -> Result<i64, String> {
    match value {
        Some(raw) => match parse_js_int(raw) {
            Some(value) if value > 0 => Ok(value),
            _ if default == DEFAULT_PAGE_SIZE => {
                Err("offset must be a positive number greater than 0".to_string())
            }
            _ => Err("page must be a positive number greater than 0".to_string()),
        },
        None => Ok(default),
    }
}

fn parse_js_int(value: &str) -> Option<i64> {
    let trimmed = value.trim_start();
    if trimmed.is_empty() {
        return None;
    }

    let mut chars = trimmed.chars().peekable();
    let mut parsed = String::new();

    if matches!(chars.peek(), Some('+') | Some('-')) {
        parsed.push(chars.next()?);
    }

    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() {
            parsed.push(ch);
            let _ = chars.next();
        } else {
            break;
        }
    }

    if parsed.is_empty() || parsed == "+" || parsed == "-" {
        return None;
    }

    parsed.parse::<i64>().ok()
}

fn normalized_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn decode_metadata(full_description: &str) -> Result<ProposalMetadata, String> {
    let mut proposal_type = ProposalType::SimpleMajority;
    let mut approval_threshold = SIMPLE_MAJORITY_BPS;
    let mut quorum = DEFAULT_QUORUM_PERCENTAGE_BPS.to_string();

    if !full_description.starts_with(METADATA_PREFIX) {
        return Ok(ProposalMetadata {
            proposal_type,
            quorum,
            approval_threshold,
        });
    }

    let Some(remaining) = full_description.get(METADATA_PREFIX.len()..) else {
        return Ok(ProposalMetadata {
            proposal_type,
            quorum,
            approval_threshold,
        });
    };
    let Some(raw_version) = remaining.get(..METADATA_VERSION.len()) else {
        return Ok(ProposalMetadata {
            proposal_type,
            quorum,
            approval_threshold,
        });
    };

    if raw_version != METADATA_VERSION {
        return Ok(ProposalMetadata {
            proposal_type,
            quorum,
            approval_threshold,
        });
    }

    let Some(without_version) = remaining.get(METADATA_VERSION.len()..) else {
        return Ok(ProposalMetadata {
            proposal_type,
            quorum,
            approval_threshold,
        });
    };

    let Some(last_pipe_index) = without_version.rfind('|') else {
        return Ok(ProposalMetadata {
            proposal_type,
            quorum,
            approval_threshold,
        });
    };

    let metadata_string = &without_version[last_pipe_index + 1..];

    for pair in metadata_string.split(',') {
        let mut parts = pair.splitn(2, '=');
        let Some(key) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else {
            continue;
        };

        if key == "approval_threshold" {
            let Some(parsed_threshold) = parse_js_int(value) else {
                continue;
            };
            if parsed_threshold > 0 {
                approval_threshold = parsed_threshold;
                proposal_type = match parsed_threshold {
                    SUPER_MAJORITY_BPS => ProposalType::SuperMajority,
                    SIMPLE_MAJORITY_BPS => ProposalType::SimpleMajority,
                    _ => {
                        return Err("Invalid approval threshold".to_string());
                    }
                };
            }
        }

        if key == "quorum" {
            let decimal = parse_decimal_value(value)?;
            if decimal.coefficient >= 0 {
                quorum = normalize_numeric_string(value);
            }
        }
    }

    Ok(ProposalMetadata {
        proposal_type,
        quorum,
        approval_threshold,
    })
}

fn get_derived_proposal_status(
    is_approved: bool,
    is_rejected: bool,
    voting_start_at: Option<NaiveDateTime>,
    voting_duration_ns: Option<&str>,
) -> String {
    if is_rejected {
        return "Rejected".to_string();
    }

    if !is_approved {
        return "Created".to_string();
    }

    if is_approved {
        if let (Some(start_time), Some(duration_ns)) = (voting_start_at, voting_duration_ns) {
            if let Some(duration_ms) = ns_string_to_ms(duration_ns) {
                let end_time_ms = start_time
                    .and_utc()
                    .timestamp_millis()
                    .saturating_add(duration_ms);
                let current_time_ms = Utc::now().timestamp_millis();
                if current_time_ms < end_time_ms {
                    return "Voting".to_string();
                }
                return "Finished".to_string();
            }
        }
    }

    "Unknown".to_string()
}

fn calculate_quorum_amount(
    total_venear_at_approval: Option<&str>,
    metadata: &ProposalMetadata,
) -> String {
    let is_prod = env::var("AGORA_ENV").ok().as_deref() == Some("prod");
    let floor_value = if is_prod {
        PROD_QUORUM_FLOOR.to_string()
    } else {
        env::var("QUORUM_FLOOR").unwrap_or_else(|_| NON_PROD_QUORUM_FLOOR.to_string())
    };

    let quorum_floor = parse_integer_i128(floor_value.as_str()).unwrap_or(0);
    let total_venear = total_venear_at_approval
        .and_then(parse_integer_i128)
        .unwrap_or(0);

    let percentage_based_quorum = parse_decimal_value(metadata.quorum.as_str())
        .ok()
        .and_then(|quorum| checked_decimal_mul_div(quorum, total_venear, 10_000))
        .unwrap_or(0);

    percentage_based_quorum.max(quorum_floor).to_string()
}

fn parse_decimal_value(raw: &str) -> Result<DecimalValue, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("empty decimal string".to_string());
    }

    let negative = trimmed.starts_with('-');
    let unsigned = trimmed.strip_prefix(['+', '-']).unwrap_or(trimmed);
    let mut parts = unsigned.split('.');
    let whole = parts.next().unwrap_or_default();
    let fractional = parts.next();
    if parts.next().is_some() {
        return Err("invalid decimal string".to_string());
    }

    if whole.is_empty() && fractional.unwrap_or_default().is_empty() {
        return Err("invalid decimal string".to_string());
    }

    if !whole.chars().all(|ch| ch.is_ascii_digit())
        || fractional
            .unwrap_or_default()
            .chars()
            .any(|ch| !ch.is_ascii_digit())
    {
        return Err("invalid decimal string".to_string());
    }

    let scale = fractional.map(|part| part.len()).unwrap_or_default() as u32;
    let combined = match fractional {
        Some(part) => format!("{whole}{part}"),
        None => whole.to_string(),
    };
    let mut coefficient = combined.parse::<i128>().map_err(|err| err.to_string())?;
    if negative {
        coefficient = -coefficient;
    }

    Ok(DecimalValue { coefficient, scale })
}

fn checked_decimal_mul_div(value: DecimalValue, rhs: i128, divisor: i128) -> Option<i128> {
    let denominator = 10_i128.checked_pow(value.scale)?.checked_mul(divisor)?;
    value.coefficient.checked_mul(rhs)?.checked_div(denominator)
}

fn parse_integer_i128(raw: &str) -> Option<i128> {
    raw.trim().parse::<i128>().ok()
}

fn ns_string_to_ms(raw: &str) -> Option<i64> {
    parse_integer_i128(raw)
        .and_then(|value| value.checked_div(1_000_000))
        .and_then(|value| i64::try_from(value).ok())
}

fn normalize_numeric_string_opt(value: Option<String>) -> Option<String> {
    value.map(|value| normalize_numeric_string(value.as_str()))
}

fn normalize_numeric_string(value: &str) -> String {
    if let Some((whole, frac)) = value.split_once('.') {
        let trimmed_frac = frac.trim_end_matches('0');
        if trimmed_frac.is_empty() {
            whole.to_string()
        } else {
            format!("{whole}.{trimmed_frac}")
        }
    } else {
        value.to_string()
    }
}

fn legacy_transaction_type(method_name: &str, event_type: &str) -> &'static str {
    match (method_name, event_type) {
        ("on_lockup_update", "on_lockup_update_ft_burn") => "unlock",
        ("on_lockup_update", "on_lockup_update_ft_mint") => "lock",
        ("delegate_all", "delegate_all_ft_burn") => "outbound_delegation",
        ("on_lockup_deployed", "lockup_deployed") => "initial_registration",
        ("delegate_all", "delegate_all_ft_mint") => "inbound_delegation",
        ("withdraw_from_staking_pool", _) | ("withdraw_all_from_staking_pool", _) => {
            "staking_pool_withdraw"
        }
        ("unstake", _) | ("unstake_all", _) => "unstake",
        _ => "unknown",
    }
}

fn format_naive_datetime(value: NaiveDateTime) -> String {
    value.format(DATE_TIME_FORMAT).to_string()
}

fn format_naive_date(value: NaiveDate) -> String {
    value
        .and_hms_milli_opt(0, 0, 0, 0)
        .expect("valid midnight")
        .format(DATE_TIME_FORMAT)
        .to_string()
}

fn naive_datetime_to_ns_string(value: NaiveDateTime) -> String {
    value
        .and_utc()
        .timestamp_millis()
        .saturating_mul(1_000_000)
        .to_string()
}

fn require_db(app_state: &AppState) -> Result<Arc<DatabaseConnection>, Response> {
    app_state.db.clone().ok_or_else(|| {
        json_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Database connection unavailable",
        )
    })
}

async fn scalar_i64_with_values(
    db: &DatabaseConnection,
    sql: &str,
    values: Vec<Value>,
) -> Result<i64, String> {
    let result = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            values,
        ))
        .await
        .map_err(|err| err.to_string())?;

    let Some(result) = result else {
        return Ok(0);
    };

    result.try_get("", "value").map_err(|err| err.to_string())
}

fn decode_row_col<T: sea_orm::TryGetable>(
    row: &QueryResult,
    column: &str,
    entity_label: &str,
) -> Result<T, String> {
    row.try_get("", column)
        .map_err(|err| format!("failed to decode {entity_label} column '{column}': {err}"))
}

fn json_ok<T: Serialize>(payload: T) -> Response {
    Json(payload).into_response()
}

fn json_error(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(AgoraLegacyErrorBody {
            error: message.into(),
        }),
    )
        .into_response()
}

fn json_message(status: StatusCode, message: impl Into<String>) -> Response {
    (
        status,
        Json(AgoraLegacyMessageBody {
            message: message.into(),
        }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_int_parser_matches_legacy_expectations() {
        assert_eq!(parse_js_int("10"), Some(10));
        assert_eq!(parse_js_int("10px"), Some(10));
        assert_eq!(parse_js_int("-5"), Some(-5));
        assert_eq!(parse_js_int("abc"), None);
    }

    #[test]
    fn metadata_decoder_defaults_to_v0() {
        let metadata = decode_metadata("plain text").expect("metadata");
        assert_eq!(metadata.proposal_type.as_str(), "SimpleMajority");
        assert_eq!(metadata.quorum, "3500");
        assert_eq!(metadata.approval_threshold, 5000);
    }

    #[test]
    fn quorum_calculation_uses_non_prod_floor_by_default() {
        let metadata = ProposalMetadata {
            proposal_type: ProposalType::SimpleMajority,
            quorum: "3500".to_string(),
            approval_threshold: 5000,
        };

        assert_eq!(
            calculate_quorum_amount(Some("1000"), &metadata),
            NON_PROD_QUORUM_FLOOR
        );
    }

    #[test]
    fn status_derivation_matches_legacy_contract() {
        assert_eq!(
            get_derived_proposal_status(false, false, None, None),
            "Created"
        );
        assert_eq!(
            get_derived_proposal_status(true, true, None, None),
            "Rejected"
        );
        assert_eq!(
            get_derived_proposal_status(true, false, None, None),
            "Unknown"
        );
    }
}
