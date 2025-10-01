use api::errors::{ApiError, ApiErrorResponse};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use common::errors::{ConfigError, DatabaseError, ServiceError, ValidationError};
use serde_json::Value;

#[test]
fn constructors_set_status_and_details() {
    let not_found = ApiErrorResponse::not_found("resource").with_details("missing");
    assert_eq!(not_found.status, StatusCode::NOT_FOUND);
    assert_eq!(not_found.error.code, "NOT_FOUND");
    assert_eq!(not_found.error.message, "resource");
    assert_eq!(not_found.error.details.as_deref(), Some("missing"));

    let forbidden = ApiErrorResponse::forbidden("no access");
    assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
    assert_eq!(forbidden.error.code, "FORBIDDEN");
    assert_eq!(forbidden.error.message, "no access");
    assert!(forbidden.error.details.is_none());
}

#[tokio::test]
async fn into_response_serializes_error_payload() {
    let response = ApiErrorResponse::internal_server_error("boom").with_details("trace");
    let http_response = response.into_response();
    assert_eq!(http_response.status(), StatusCode::INTERNAL_SERVER_ERROR);

    let body = axum::body::to_bytes(http_response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let payload: ApiError = serde_json::from_slice(&body).expect("valid api error json");

    assert_eq!(payload.code, "INTERNAL_ERROR");
    assert_eq!(payload.message, "boom");
    assert_eq!(payload.details.as_deref(), Some("trace"));
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
        .message
        .contains("Widget with id 42 not found"));

    let connection = DatabaseError::ConnectionFailed {
        message: "timeout".to_string(),
    };
    let connection_response: ApiErrorResponse = connection.into();
    assert_eq!(
        connection_response.status,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(connection_response.error.code, "INTERNAL_ERROR");
    assert_eq!(
        connection_response.error.details.as_deref(),
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
    assert_eq!(response.error.code, "VALIDATION_ERROR");
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
        .details
        .as_deref()
        .expect("details")
        .contains("DATABASE_URL"));

    let service_error = ServiceError::ServiceUnavailable {
        service_name: "search".to_string(),
    };
    let service_response: ApiErrorResponse = service_error.into();
    assert_eq!(service_response.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(service_response.error.code, "SERVICE_UNAVAILABLE");
}

#[test]
fn anyhow_downcasts_and_fallbacks() {
    let typed = anyhow::anyhow!(DatabaseError::QueryFailed {
        message: "syntax".to_string(),
    });
    let typed_response: ApiErrorResponse = typed.into();
    assert_eq!(typed_response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        typed_response.error.details.as_deref(),
        Some("Query execution failed: syntax")
    );

    let generic = anyhow::anyhow!("unhandled");
    let generic_response: ApiErrorResponse = generic.into();
    assert_eq!(generic_response.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(generic_response.error.code, "INTERNAL_ERROR");
    assert_eq!(generic_response.error.details.as_deref(), Some("unhandled"));
}

#[test]
fn api_error_serializes_optional_fields() {
    let error = ApiError::new("CODE", "msg");
    let value = serde_json::to_value(&error).expect("serialize");
    assert_eq!(value.get("details"), None);

    let with_details = error.with_details("extra");
    let value = serde_json::to_value(&with_details).expect("serialize with details");
    assert_eq!(
        value.get("details"),
        Some(&Value::String("extra".to_string()))
    );
}
