use api::handlers::health::health_check;
use api::state::AppState;
use axum::extract::State;
use sea_orm::{DatabaseBackend, DbErr, MockDatabase, MockExecResult, RuntimeErr};

#[path = "../support/mod.rs"]
mod support;

#[tokio::test]
async fn health_check_returns_healthy_when_db_exec_succeeds() {
    support::init_test_tracing();

    let db = MockDatabase::new(DatabaseBackend::MySql)
        .append_exec_results([MockExecResult {
            last_insert_id: 0,
            rows_affected: 1,
        }])
        .into_connection();

    let result = health_check(State(AppState::from_database(Some(db)))).await;

    let payload = match result {
        Ok(json) => json.0,
        Err(err) => panic!("expected Ok, got error: {}", err.error.message),
    };

    assert_eq!(payload.status, "healthy");
    assert!(payload.database.connected);
    assert!(payload.database.response_time_ms.is_some());
    assert!(payload.database.error.is_none());
    assert_eq!(payload.version, env!("CARGO_PKG_VERSION"));
}

#[tokio::test]
async fn health_check_returns_degraded_when_db_query_fails() {
    support::init_test_tracing();

    let db = MockDatabase::new(DatabaseBackend::MySql)
        .append_exec_errors([DbErr::Exec(RuntimeErr::Internal("boom".into()))])
        .into_connection();

    let result = health_check(State(AppState::from_database(Some(db)))).await;

    let payload = match result {
        Ok(json) => json.0,
        Err(err) => panic!("expected Ok, got error: {}", err.error.message),
    };

    assert_eq!(payload.status, "degraded");
    assert!(!payload.database.connected);
    assert!(payload.database.response_time_ms.is_some());
    assert!(payload
        .database
        .error
        .as_deref()
        .expect("database error message")
        .contains("Database connection error"));
}

#[tokio::test]
async fn health_check_returns_degraded_without_database() {
    support::init_test_tracing();

    let result = health_check(State(AppState::new(None))).await;

    let payload = match result {
        Ok(json) => json.0,
        Err(err) => panic!("expected Ok, got error: {}", err.error.message),
    };

    assert_eq!(payload.status, "degraded");
    assert!(!payload.database.connected);
    assert!(payload.database.response_time_ms.is_none());
    assert_eq!(
        payload.database.error.as_deref(),
        Some("Database connection not configured")
    );
}
