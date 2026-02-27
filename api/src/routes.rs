//! API route definitions

use crate::state::AppState;
use axum::Router;

pub mod discourse;
pub mod health;

/// Create all API routes
pub fn create_routes() -> Router<AppState> {
    Router::new().merge(discourse::discourse_routes())
}
