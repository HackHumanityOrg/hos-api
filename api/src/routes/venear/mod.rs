//! veNEAR governance API routes.

use crate::{handlers::venear, state::AppState};
use axum::{routing::get, Router};

pub fn venear_routes() -> Router<AppState> {
    Router::new()
        .route("/venear/config", get(venear::get_config))
        .route("/venear/config/history", get(venear::list_config_history))
        .route("/venear/proposals", get(venear::list_proposals))
        .route("/venear/proposals/{proposal_id}", get(venear::get_proposal))
        .route(
            "/venear/proposals/{proposal_id}/timeline",
            get(venear::list_proposal_timeline),
        )
        .route(
            "/venear/proposals/{proposal_id}/votes",
            get(venear::list_proposal_votes),
        )
        .route(
            "/venear/proposals/{proposal_id}/vote_history",
            get(venear::list_proposal_vote_history),
        )
        .route("/venear/accounts", get(venear::list_accounts))
        .route("/venear/accounts/{account_id}", get(venear::get_account))
        .route(
            "/venear/accounts/{account_id}/proposals",
            get(venear::list_account_proposals),
        )
        .route(
            "/venear/accounts/{account_id}/votes",
            get(venear::list_account_votes),
        )
        .route(
            "/venear/accounts/{account_id}/vote_history",
            get(venear::list_account_vote_history),
        )
        .route(
            "/venear/accounts/{account_id}/voting_power_history",
            get(venear::list_account_voting_power_history),
        )
        .route(
            "/venear/accounts/{account_id}/delegation_history",
            get(venear::list_account_delegation_history),
        )
        .route(
            "/venear/accounts/{account_id}/lockup_events",
            get(venear::list_account_lockup_events),
        )
        .route(
            "/venear/accounts/{account_id}/rewards_events",
            get(venear::list_account_rewards_events),
        )
        .route("/venear/delegations", get(venear::list_delegations))
        .route("/venear/lockups", get(venear::list_lockups))
        .route(
            "/venear/lockups/{lockup_account_id}",
            get(venear::get_lockup),
        )
        .route(
            "/venear/lockups/{lockup_account_id}/events",
            get(venear::list_lockup_events),
        )
        .route("/venear/admin/actions", get(venear::list_admin_actions))
        .route("/venear/activity", get(venear::list_activity))
}
