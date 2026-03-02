//! API route definitions

use crate::state::AppState;
use axum::Router;

pub mod discourse;
pub mod health;
pub mod telegram;
pub mod venear;

/// Create all API routes
pub fn create_routes() -> Router<AppState> {
    Router::new()
        .merge(discourse::discourse_routes())
        .merge(telegram::telegram_routes())
        .merge(venear::venear_routes())
}
