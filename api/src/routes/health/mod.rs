//! Health routes module

use crate::{handlers::health, state::AppState};
use axum::{routing::get, Router};

/// Create health routes
pub fn health_routes() -> Router<AppState> {
    Router::new().route("/health", get(health::health_check))
}
