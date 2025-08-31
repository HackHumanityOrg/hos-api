//! API route definitions

use crate::state::AppState;
use axum::Router;

pub mod health;

/// Create all API routes
pub fn create_routes() -> Router<AppState> {
    Router::new()
    // Additional API routes will be nested here as modules are implemented
    // For example: .nest("/users", user::user_routes())
    // Health check is at root level, not nested under /api/v1
}
