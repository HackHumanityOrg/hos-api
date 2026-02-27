use api::errors::{ApiError, ApiErrorResponse};
use axum::http::{header::CONTENT_TYPE, StatusCode};
use axum::response::IntoResponse;
use common::errors::{ConfigError, DatabaseError, ServiceError, ValidationError};
use serde_json::Value;

#[test]
fn constructors_set_status_and_problem_fields() {
    let not_found = ApiErrorResponse::not_found("resource").with_detail("missing");
    assert_eq!(not_found.status, StatusCode::NOT_FOUND);
    assert_eq!(not_found.error.code.as_deref(), Some("NOT_FOUND"));
    assert_eq!(not_found.error.title, "resource");
    assert_eq!(not_found.error.detail.as_deref(), Some("missing"));
    assert_eq!(not_found.error.status, 404);

    let forbidden = ApiErrorResponse::forbidden("no access");
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    assert_eq!(forbidden.error.code.as_deref(), Some("FORBIDDEN"));
    assert_eq!(forbidden.error.title, "no access");
    assert!(forbidden.error.detail.is_none());
}

#[tokio::test]
async fn into_response_serializes_problem_json() {
    let response = ApiErrorResponse::internal_server_error("boom").with_detail("trace");
    let http_response = response.into_response();
    assert_eq!(http_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        http_response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("application/problem+json")
    );

    let body = axum::body::to_bytes(http_response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let payload: ApiError = serde_json::from_slice(&body).expect("valid api error json");

    assert_eq!(payload.code.as_deref(), Some("INTERNAL_ERROR"));
    assert_eq!(payload.title, "boom");
    assert_eq!(payload.detail.as_deref(), Some("trace"));
    assert_eq!(payload.status, 500);
}

#[test]
fn database_error_mappings() {
    let missing = DatabaseError::EntityNotFound {
        entity_type: "Widget".to_string(),
        id: "42".to_string(),
    };
    let missing_response: ApiErrorResponse = missing.clone().into();
    assert_eq!(missing_response.status, StatusCode::NOT_FOUND);
    assert!(missing_response
        .error
        .title
        .contains("Widget with id 42 not found"));

    let connection = DatabaseError::ConnectionFailed {
        message: "timeout".to_string(),
    };
    let connection_response: ApiErrorResponse = connection.into();
    assert_eq!(connection_response.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        connection_response.error.code.as_deref(),
        Some("SERVICE_UNAVAILABLE")
    );
    assert_eq!(
        connection_response.error.detail.as_deref(),
        Some("Connection failed: timeout")
    );
}

#[test]
fn validation_error_maps_to_422() {
    let validation = ValidationError::InvalidField {
        field: "email".to_string(),
        value: "not-an-email".to_string(),
    };
    let response: ApiErrorResponse = validation.into();
    assert_eq!(response.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(response.error.code.as_deref(), Some("VALIDATION_ERROR"));
}

#[test]
fn config_and_service_error_mappings() {
    let config_error = ConfigError::MissingEnvVar {
        var_name: "DATABASE_URL".to_string(),
    };
    let config_response: ApiErrorResponse = config_error.into();
    assert_eq!(config_response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(config_response
        .error
        .detail
        .as_deref()
        .expect("detail")
        .contains("DATABASE_URL"));

    let service_error = ServiceError::ServiceUnavailable {
        service_name: "search".to_string(),
    };
    let service_response: ApiErrorResponse = service_error.into();
    assert_eq!(service_response.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        service_response.error.code.as_deref(),
        Some("SERVICE_UNAVAILABLE")
    );
}

#[test]
fn anyhow_downcasts_and_fallbacks() {
    let typed = anyhow::anyhow!(DatabaseError::QueryFailed {
        message: "syntax".to_string(),
    });
    let typed_response: ApiErrorResponse = typed.into();
    assert_eq!(typed_response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        typed_response.error.detail.as_deref(),
        Some("Query execution failed: syntax")
    );

    let generic = anyhow::anyhow!("unhandled");
    let generic_response: ApiErrorResponse = generic.into();
    assert_eq!(generic_response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(generic_response.error.code.as_deref(), Some("INTERNAL_ERROR"));
    assert_eq!(generic_response.error.detail.as_deref(), Some("unhandled"));
}

#[test]
fn api_error_serializes_optional_fields() {
    let error = ApiError::new("https://example.test/problem", "msg", StatusCode::BAD_REQUEST);
    let value = serde_json::to_value(&error).expect("serialize");
    assert_eq!(value.get("detail"), None);
    assert_eq!(value.get("instance"), None);

    let with_detail = error.with_detail("extra").with_code("BAD_REQUEST");
    let value = serde_json::to_value(&with_detail).expect("serialize with details");
    assert_eq!(
        value.get("detail"),
        Some(&Value::String("extra".to_string()))
    );
    assert_eq!(
        value.get("code"),
        Some(&Value::String("BAD_REQUEST".to_string()))
    );
}
