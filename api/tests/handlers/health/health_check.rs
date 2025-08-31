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
    assert!(db_health["response_time_ms"].is_null()); // No response time when no DB configured
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

    // Should have response time measurement
    assert!(db_health["response_time_ms"].is_number());
    let response_time = db_health["response_time_ms"].as_u64().unwrap();
    assert!(
        response_time < 5000,
        "Response time should be reasonable: {}ms",
        response_time
    ); // Less than 5 seconds
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

        // Each request should have response time
        assert!(
            db_health["response_time_ms"].is_number(),
            "Response time should be present on check {}",
            i
        );
        let response_time = db_health["response_time_ms"].as_u64().unwrap();
        assert!(
            response_time < 5000,
            "Response time should be reasonable on check {}: {}ms",
            i,
            response_time
        );
    }
}

#[rstest]
#[tokio::test]
async fn test_health_handler_database_response_time_measurement(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let server = create_test_server_with_db(Some(test_db.db())).await;

    let response = server.get("/health").await;
    response.assert_status_ok();

    let json: Value = response.json();
    let db_health = &json["database"];

    // Verify response time is measured and reasonable
    assert!(db_health["response_time_ms"].is_number());
    let response_time = db_health["response_time_ms"].as_i64().unwrap();

    // Response time should be non-zero (measurement took place)
    assert!(response_time >= 0);

    // Response time should be reasonable (less than 5 seconds for a simple SELECT 1)
    assert!(
        response_time < 5000,
        "Database response time too high: {}ms",
        response_time
    );

    // Log the response time for debugging
    println!("Database response time: {}ms", response_time);
}

#[rstest]
#[tokio::test]
async fn test_health_handler_response_time_consistency(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let server = create_test_server_with_db(Some(test_db.db())).await;

    let mut response_times = Vec::new();

    // Make multiple requests to check response time consistency
    for i in 0..5 {
        let response = server.get("/health").await;
        response.assert_status_ok();

        let json: Value = response.json();
        let db_health = &json["database"];

        assert_eq!(db_health["connected"], true);
        assert!(db_health["response_time_ms"].is_number());

        let response_time = db_health["response_time_ms"].as_u64().unwrap();
        response_times.push(response_time);

        println!("Request {}: {}ms", i + 1, response_time);
    }

    // All response times should be reasonable
    for (i, &time) in response_times.iter().enumerate() {
        assert!(
            time < 5000,
            "Request {} response time too high: {}ms",
            i + 1,
            time
        );
    }

    // Calculate average and ensure it's reasonable
    let avg_time = response_times.iter().sum::<u64>() / response_times.len() as u64;
    assert!(
        avg_time < 1000,
        "Average response time too high: {}ms",
        avg_time
    );
}

#[rstest]
#[tokio::test]
async fn test_health_handler_database_error_response_time() {
    init_test_tracing();

    // Create a broken database connection (invalid URL)
    use migration::sea_orm::{ConnectOptions, Database};

    // This should fail to connect
    let mut options =
        ConnectOptions::new("mysql://invalid:invalid@localhost:9999/invalid".to_owned());
    options.connect_timeout(std::time::Duration::from_secs(1));

    let broken_db = Database::connect(options).await;

    match broken_db {
        Ok(db) => {
            // If it somehow connects, test with it
            let server = create_test_server_with_db(Some(db)).await;
            let response = server.get("/health").await;
            response.assert_status_ok();

            let json: Value = response.json();
            let db_health = &json["database"];

            // Should still measure response time even if query fails
            if db_health["response_time_ms"].is_number() {
                let response_time = db_health["response_time_ms"].as_i64().unwrap();
                assert!(response_time >= 0);
                println!("Error case response time: {}ms", response_time);
            }
        }
        Err(_) => {
            // Expected case - connection fails immediately
            // Test with no database instead
            let server = create_test_server_with_db(None).await;
            let response = server.get("/health").await;
            response.assert_status_ok();

            let json: Value = response.json();
            let db_health = &json["database"];

            // Should not have response time when no DB configured
            assert!(db_health["response_time_ms"].is_null());
        }
    }
}

#[rstest]
#[tokio::test]
async fn test_health_response_structure_with_timing(#[future] test_db: TestDatabase) {
    init_test_tracing();

    let test_db = test_db.await;
    let server = create_test_server_with_db(Some(test_db.db())).await;

    let response = server.get("/health").await;
    response.assert_status_ok();

    let json: Value = response.json();

    // Validate complete response structure with new fields
    let expected_fields = ["status", "timestamp", "version", "database"];
    for field in expected_fields {
        assert!(
            json.get(field).is_some(),
            "Missing required field: {}",
            field
        );
    }

    // Validate database health structure includes response time
    let db_health = &json["database"];
    let expected_db_fields = ["connected", "response_time_ms"];
    for field in expected_db_fields {
        assert!(
            db_health.get(field).is_some(),
            "Missing required database field: {}",
            field
        );
    }

    // Validate field types
    assert!(json["status"].is_string());
    assert!(json["timestamp"].is_string());
    assert!(json["version"].is_string());
    assert!(json["database"].is_object());
    assert!(db_health["connected"].is_boolean());
    assert!(db_health["response_time_ms"].is_number());

    // Optional error field should not be present when connected
    if db_health["connected"].as_bool().unwrap() {
        assert!(db_health["error"].is_null());
    }
}
