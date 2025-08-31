// Unit/integration tests for health check handler
use axum_test::TestServer;
use migration::sea_orm::DatabaseConnection;
use rstest::*;
use serde_json::Value;

use crate::common::{init_test_tracing, TestDatabase};

/// Create a test server with the health check handler
async fn create_test_server_with_db(db: Option<DatabaseConnection>) -> TestServer {
    use api::handlers::health;
    use api::state::AppState;
    use axum::{routing::get, Router};

    let app_state = AppState { db };

    let app = Router::new()
        .route("/health", get(health::health_check))
        .with_state(app_state);

    TestServer::new(app).unwrap()
}

#[rstest]
#[tokio::test]
async fn test_health_handler_without_database() {
    init_test_tracing();

    // Create server without database connection
    let server = create_test_server_with_db(None).await;

    let response = server.get("/health").await;

    // Should return degraded status when no database
    response.assert_status_ok();

    let json: Value = response.json();

    assert_eq!(json["status"], "degraded");
    assert!(json.get("timestamp").is_some());
    assert_eq!(json["version"], "0.1.0");

    // Database health should show not connected
    let db_health = &json["database"];
    assert_eq!(db_health["connected"], false);
    assert!(db_health["error"].is_string());
}

#[rstest]
#[tokio::test]
async fn test_health_handler_response_structure() {
    init_test_tracing();

    let server = create_test_server_with_db(None).await;

    let response = server.get("/health").await;
    response.assert_status_ok();

    let json: Value = response.json();

    // Validate response structure
    let expected_fields = ["status", "timestamp", "version", "database"];
    for field in expected_fields {
        assert!(
            json.get(field).is_some(),
            "Missing required field: {}",
            field
        );
    }

    // Validate field types
    assert!(json["status"].is_string());
    assert!(json["timestamp"].is_string());
    assert!(json["version"].is_string());
    assert!(json["database"].is_object());
}

#[fixture]
async fn test_db() -> TestDatabase {
    TestDatabase::new().await
}

#[rstest]
#[tokio::test]
async fn test_health_handler_with_database(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let server = create_test_server_with_db(Some(test_db.db())).await;

    let response = server.get("/health").await;
    response.assert_status_ok();

    let json: Value = response.json();

    // Should be healthy with database connection
    assert_eq!(json["status"], "healthy");
    assert!(json.get("timestamp").is_some());
    assert_eq!(json["version"], "0.1.0");

    // Database health should show connected
    let db_health = &json["database"];
    assert_eq!(db_health["connected"], true);
    assert!(db_health["error"].is_null());
}

#[rstest]
#[tokio::test]
async fn test_health_handler_database_security(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let server = create_test_server_with_db(Some(test_db.db())).await;

    let response = server.get("/health").await;
    response.assert_status_ok();

    let json: Value = response.json();

    // Should be healthy with database connection
    assert_eq!(json["status"], "healthy");

    let db_health = &json["database"];
    assert_eq!(db_health["connected"], true);

    // Validate that no sensitive information is exposed
    let json_str = json.to_string();
    assert!(!json_str.contains("password"));
    assert!(!json_str.contains("user"));
    assert!(!json_str.contains("mysql://"));
    assert!(!json_str.contains("3306"));
}

#[rstest]
#[tokio::test]
async fn test_health_handler_database_connection_persistence(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let server = create_test_server_with_db(Some(test_db.db())).await;

    // Make multiple requests to ensure connection persistence
    for i in 0..3 {
        let response = server.get("/health").await;
        response.assert_status_ok();

        let json: Value = response.json();
        assert_eq!(
            json["status"], "healthy",
            "Health check {} should be healthy",
            i
        );

        let db_health = &json["database"];
        assert_eq!(
            db_health["connected"], true,
            "Database should be connected on check {}",
            i
        );
        assert!(
            db_health["error"].is_null(),
            "No error should be present on check {}",
            i
        );
    }
}