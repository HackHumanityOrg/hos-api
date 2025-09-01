use axum::http::StatusCode;
use common::errors::{ConfigError, DatabaseError, ServiceError, ValidationError};

// ApiErrorResponse is re-exported by the api crate
use api::ApiErrorResponse;

#[test]
fn test_database_error_entity_not_found_maps_to_404() {
    let err = DatabaseError::EntityNotFound {
        entity_type: "User".to_string(),
        id: "123".to_string(),
    };
    let resp: ApiErrorResponse = err.into();
    assert_eq!(resp.status, StatusCode::NOT_FOUND);
    assert_eq!(resp.error.code, "NOT_FOUND");
    assert!(resp.error.message.contains("User"));
}

#[test]
fn test_database_error_connection_failed_maps_to_500() {
    let err = DatabaseError::ConnectionFailed {
        message: "cannot connect".to_string(),
    };
    let resp: ApiErrorResponse = err.into();
    assert_eq!(resp.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(resp.error.code, "INTERNAL_ERROR");
    assert!(resp.error.details.is_some());
}

#[test]
fn test_validation_error_unprocessable_entity() {
    let err = ValidationError::RequiredField {
        field: "name".to_string(),
    };
    let resp: ApiErrorResponse = err.into();
    assert_eq!(resp.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(resp.error.code, "VALIDATION_ERROR");
}

#[test]
fn test_service_error_mappings() {
    // ServiceUnavailable -> 503
    let resp: ApiErrorResponse = ServiceError::ServiceUnavailable {
        service_name: "search".to_string(),
    }
    .into();
    assert_eq!(resp.status, StatusCode::SERVICE_UNAVAILABLE);

    // RequestTimeout -> 408
    let resp: ApiErrorResponse = ServiceError::RequestTimeout { timeout_ms: 1000 }.into();
    assert_eq!(resp.status, StatusCode::REQUEST_TIMEOUT);

    // AuthenticationFailed -> 401
    let resp: ApiErrorResponse = ServiceError::AuthenticationFailed.into();
    assert_eq!(resp.status, StatusCode::UNAUTHORIZED);

    // AuthorizationFailed -> 403
    let resp: ApiErrorResponse = ServiceError::AuthorizationFailed.into();
    assert_eq!(resp.status, StatusCode::FORBIDDEN);
}

#[test]
fn test_config_error_maps_to_500() {
    let err = ConfigError::ParseError {
        message: "bad toml".to_string(),
    };
    let resp: ApiErrorResponse = err.into();
    assert_eq!(resp.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(resp.error.code, "INTERNAL_ERROR");
}

#[test]
fn test_anyhow_fallback_internal() {
    let err = anyhow::anyhow!("boom");
    let resp: ApiErrorResponse = err.into();
    assert_eq!(resp.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(resp.error.code, "INTERNAL_ERROR");
    assert!(resp.error.details.as_deref().unwrap().contains("boom"));
}

#[test]
fn test_anyhow_downcast_to_database_error() {
    let err = anyhow::Error::from(DatabaseError::EntityNotFound {
        entity_type: "Widget".to_string(),
        id: "A1".to_string(),
    });
    let resp: ApiErrorResponse = err.into();
    assert_eq!(resp.status, StatusCode::NOT_FOUND);
    assert_eq!(resp.error.code, "NOT_FOUND");
}
