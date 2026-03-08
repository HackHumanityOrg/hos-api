//! API route definitions

use crate::state::AppState;
use axum::Router;

pub mod agora;
pub mod discourse;
pub mod health;
pub mod identity;
pub mod telegram;
pub mod venear;

/// Create all API routes
pub fn create_routes() -> Router<AppState> {
    Router::new()
        .merge(discourse::discourse_routes())
        .merge(identity::identity_routes())
        .merge(telegram::telegram_routes())
        .merge(venear::venear_routes())
}
