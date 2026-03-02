//! Identity API routes.

use crate::{handlers::identity, state::AppState};
use axum::{routing::get, Router};

pub fn identity_routes() -> Router<AppState> {
    Router::new()
        .route("/identity/persons", get(identity::list_persons))
        .route(
            "/identity/persons/{person_id}/accounts",
            get(identity::list_person_accounts),
        )
}
