use api::types::HealthResponse;
use axum::{body::Body, http::Request};
use tower::ServiceExt;

#[path = "../support/mod.rs"]
mod support;

#[tokio::test]
async fn health_endpoint_reports_healthy_with_live_database() {
    support::init_test_tracing();

    let (router, _database) = support::router_with_database().await;
    let response = router
        .clone()
        .into_service()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let payload: HealthResponse = serde_json::from_slice(&bytes).expect("health payload");

    assert_eq!(payload.status, "healthy");
    assert!(payload.database.connected);
    assert!(
        payload.database.response_time_ms.is_some(),
        "database response time should be recorded"
    );
    assert!(payload.database.error.is_none());
}

#[tokio::test]
async fn health_endpoint_gracefully_degrades_when_database_unavailable() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;

    drop(database);
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;

    let response = router
        .into_service()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .expect("response");

    assert_eq!(response.status(), axum::http::StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let payload: HealthResponse = serde_json::from_slice(&bytes).expect("health payload");

    assert_eq!(payload.status, "degraded");
    assert!(!payload.database.connected);
    assert!(payload.database.response_time_ms.is_some());
    assert!(payload
        .database
        .error
        .as_deref()
        .unwrap_or_default()
        .contains("Database connection error"));
}
