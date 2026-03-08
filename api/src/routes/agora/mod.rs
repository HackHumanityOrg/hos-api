//! Agora compatibility API routes.

use crate::{handlers::agora, state::AppState};
use axum::{routing::get, Router};

pub fn agora_routes() -> Router<AppState> {
    Router::new()
        .route("/proposal/approved", get(agora::get_approved_proposals))
        .route("/proposal/pending", get(agora::get_pending_proposals))
        .route(
            "/proposal/{proposal_id}/votes",
            get(agora::get_proposal_votes),
        )
        .route(
            "/proposal/{proposal_id}/non-voters",
            get(agora::get_proposal_non_voters),
        )
        .route(
            "/proposal/{proposal_id}/charts",
            get(agora::get_proposal_charts),
        )
        .route(
            "/proposal/{proposal_id}/quorum",
            get(agora::get_proposal_quorum),
        )
        .route("/delegates", get(agora::get_delegates))
        .route("/delegates/{address}", get(agora::get_delegate))
        .route(
            "/delegates/{address}/voting-history",
            get(agora::get_delegate_voting_history),
        )
        .route(
            "/delegates/{address}/delegated-from",
            get(agora::get_delegate_delegated_from),
        )
        .route(
            "/delegates/{address}/delegated-to",
            get(agora::get_delegate_delegated_to),
        )
        .route(
            "/delegates/{address}/hos-activity",
            get(agora::get_delegate_hos_activity),
        )
        .route(
            "/delegate_statement_changes",
            get(agora::get_delegate_statement_changes),
        )
        .route(
            "/get_voting_power_chart/{account_id}",
            get(agora::get_voting_power_chart),
        )
        .route("/vote_changes/{proposal_id}", get(agora::get_vote_changes))
}
