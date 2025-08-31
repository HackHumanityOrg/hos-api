//! API error types and error handling for HTTP responses

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use common::errors::{ConfigError, DatabaseError, ServiceError, ValidationError};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Standard API error response structure
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ApiError {
    /// Error code for programmatic handling
    pub code: String,
    /// Human-readable error message
    pub message: String,
    /// Optional additional details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl ApiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            details: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

/// API error response with status code
pub struct ApiErrorResponse {
    pub status: StatusCode,
    pub error: ApiError,
}

impl ApiErrorResponse {
    pub fn new(status: StatusCode, code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            status,
            error: ApiError::new(code, message),
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.error = self.error.with_details(details);
        self
    }
}

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        (self.status, Json(self.error)).into_response()
    }
}

// Convenience constructors for common error types
impl ApiErrorResponse {
    pub fn internal_server_error(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "NOT_FOUND", message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", message)
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "FORBIDDEN", message)
    }

    pub fn unprocessable_entity(message: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            message,
        )
    }
}

// Convert common error types to API responses
impl From<DatabaseError> for ApiErrorResponse {
    fn from(err: DatabaseError) -> Self {
        match err {
            DatabaseError::EntityNotFound { entity_type, id } => {
                Self::not_found(format!("{} with id {} not found", entity_type, id))
            }
            DatabaseError::ConnectionFailed { .. } => {
                Self::internal_server_error("Database connection failed")
                    .with_details(err.to_string())
            }
            DatabaseError::QueryFailed { message: _ } => {
                Self::internal_server_error("Database query failed").with_details(err.to_string())
            }
            DatabaseError::TransactionFailed { message: _ } => {
                Self::internal_server_error("Database transaction failed")
                    .with_details(err.to_string())
            }
            DatabaseError::MigrationFailed { .. } => {
                Self::internal_server_error("Database migration failed")
                    .with_details(err.to_string())
            }
        }
    }
}

impl From<ValidationError> for ApiErrorResponse {
    fn from(err: ValidationError) -> Self {
        match err {
            ValidationError::RequiredField { .. } | ValidationError::InvalidField { .. } => {
                Self::unprocessable_entity(err.to_string())
            }
        }
    }
}

impl From<ConfigError> for ApiErrorResponse {
    fn from(err: ConfigError) -> Self {
        Self::internal_server_error("Configuration error").with_details(err.to_string())
    }
}

impl From<ServiceError> for ApiErrorResponse {
    fn from(err: ServiceError) -> Self {
        match err {
            ServiceError::ServiceUnavailable { .. } => Self::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "SERVICE_UNAVAILABLE",
                err.to_string(),
            ),
            ServiceError::RequestTimeout { .. } => Self::new(
                StatusCode::REQUEST_TIMEOUT,
                "REQUEST_TIMEOUT",
                err.to_string(),
            ),
            ServiceError::InvalidResponse { .. } => Self::internal_server_error(err.to_string()),
            ServiceError::AuthenticationFailed => Self::unauthorized("Authentication failed"),
            ServiceError::AuthorizationFailed => Self::forbidden("Insufficient permissions"),
        }
    }
}

impl From<anyhow::Error> for ApiErrorResponse {
    fn from(err: anyhow::Error) -> Self {
        // Try to downcast to known error types first
        if let Some(db_err) = err.downcast_ref::<DatabaseError>() {
            return (*db_err).clone().into();
        }
        if let Some(val_err) = err.downcast_ref::<ValidationError>() {
            return (*val_err).clone().into();
        }
        if let Some(cfg_err) = err.downcast_ref::<ConfigError>() {
            return (*cfg_err).clone().into();
        }
        if let Some(svc_err) = err.downcast_ref::<ServiceError>() {
            return (*svc_err).clone().into();
        }

        // Fallback to generic internal server error
        Self::internal_server_error("Internal server error").with_details(err.to_string())
    }
}
