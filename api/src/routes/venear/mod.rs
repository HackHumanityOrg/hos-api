//! veNEAR governance API routes.

use crate::{handlers::venear, state::AppState};
use axum::{routing::get, Router};

pub fn venear_routes() -> Router<AppState> {
    Router::new()
        .route("/venear/proposals", get(venear::list_proposals))
        .route("/venear/proposals/{proposal_id}", get(venear::get_proposal))
        .route(
            "/venear/proposals/{proposal_id}/events",
            get(venear::list_proposal_events),
        )
        .route(
            "/venear/proposals/{proposal_id}/votes",
            get(venear::list_proposal_votes),
        )
        .route("/venear/delegates", get(venear::list_delegates))
        .route(
            "/venear/delegates/{account_id}/voting-power",
            get(venear::list_delegate_voting_power),
        )
        .route(
            "/venear/delegates/{account_id}/delegations",
            get(venear::list_delegate_delegations),
        )
}
