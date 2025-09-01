// Integration tests for health routes - focusing on HTTP behavior
use axum::http::StatusCode;
use rstest::*;
use serde_json::Value;

use crate::common::{create_test_server_with_db, init_test_tracing};

/// Test server helper moved to crate::common

#[rstest]
#[tokio::test]
async fn test_health_endpoint_performance() {
    init_test_tracing();

    let server = create_test_server_with_db(None).await;

    let start = std::time::Instant::now();
    let response = server.get("/health").await;
    let duration = start.elapsed();

    response.assert_status_ok();

    // Health check should be reasonably fast
    assert!(
        duration.as_millis() < 500,
        "Health check took too long: {:?}",
        duration
    );
}

#[rstest]
#[tokio::test]
async fn test_health_endpoint_http_methods() {
    init_test_tracing();

    let server = create_test_server_with_db(None).await;

    // GET should work
    let response = server.get("/health").await;
    response.assert_status_ok();

    // POST should not be allowed
    let response = server.post("/health").await;
    response.assert_status(StatusCode::METHOD_NOT_ALLOWED);

    // PUT should not be allowed
    let response = server.put("/health").await;
    response.assert_status(StatusCode::METHOD_NOT_ALLOWED);

    // DELETE should not be allowed
    let response = server.delete("/health").await;
    response.assert_status(StatusCode::METHOD_NOT_ALLOWED);
}

#[rstest]
#[tokio::test]
async fn test_health_endpoint_json_content_type() {
    init_test_tracing();

    let server = create_test_server_with_db(None).await;

    let response = server.get("/health").await;
    response.assert_status_ok();

    // Check that content-type is JSON
    let headers = response.headers();
    let content_type = headers.get("content-type").unwrap().to_str().unwrap();

    assert!(
        content_type.starts_with("application/json"),
        "Expected JSON content-type, got: {}",
        content_type
    );
}

#[rstest]
#[tokio::test]
async fn test_concurrent_health_checks() {
    init_test_tracing();

    let server = create_test_server_with_db(None).await;

    // Run multiple concurrent requests using join_all instead of spawn
    let mut futures = Vec::new();

    for _i in 0..5 {
        let future = async {
            let response = server.get("/health").await;
            response.assert_status_ok();

            let json: Value = response.json();
            assert_eq!(json["status"], "degraded"); // No DB connection
        };
        futures.push(future);
    }

    // Wait for all requests to complete concurrently
    futures::future::join_all(futures).await;
}
