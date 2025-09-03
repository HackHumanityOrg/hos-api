// Common testing utilities and fixtures
use axum_test::TestServer;
use sea_orm::DatabaseConnection;
pub use test_support::{init_test_tracing, TestDatabase};

/// Create a test server with optional database connection using the health route
pub async fn create_test_server_with_db(db: Option<DatabaseConnection>) -> TestServer {
    use api::handlers::health;
    use api::state::AppState;
    use axum::{routing::get, Router};

    let app_state = AppState { db };

    let app = Router::new()
        .route("/health", get(health::health_check))
        .with_state(app_state);

    TestServer::new(app).unwrap()
}
