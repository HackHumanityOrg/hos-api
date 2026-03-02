//! veNEAR governance API routes.

use crate::{handlers::venear, state::AppState};
use axum::{routing::get, Router};

pub fn venear_routes() -> Router<AppState> {
    Router::new()
        .route("/venear/proposals", get(venear::list_proposals))
        .route(
            "/venear/proposals/{proposal_id}/events",
            get(venear::list_proposal_events),
        )
        .route(
            "/venear/proposals/{proposal_id}/votes",
            get(venear::list_proposal_votes),
        )
        .route(
            "/venear/accounts/{account_id}/balances",
            get(venear::list_account_balances),
        )
        .route(
            "/venear/accounts/{account_id}/delegations",
            get(venear::list_account_delegations),
        )
        .route("/venear/lockups", get(venear::list_lockups))
        .route(
            "/venear/lockups/{lockup_account_id}/actions",
            get(venear::list_lockup_actions),
        )
}
