#![allow(dead_code)]

use api::{routes, state::AppState};
use axum::Router;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

pub use test_support::{init_test_tracing, TestDatabase};

/// Build the API router with optional database connection for tests
pub fn build_router_with_state(db: Option<Arc<DatabaseConnection>>) -> Router {
    let state = AppState::new(db);
    Router::new()
        .nest("/api/v1", routes::create_routes())
        .merge(routes::health::health_routes())
        .with_state(state)
}

/// Provision a disposable MySQL instance and router for e2e tests
pub async fn router_with_database() -> (Router, TestDatabase) {
    init_test_tracing();
    let database = TestDatabase::new().await;
    let conn = database.db();
    let router = build_router_with_state(Some(conn.clone()));
    (router, database)
}
