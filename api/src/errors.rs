//! API error types and RFC 9457 Problem Details responses.

use axum::{
    http::{header::CONTENT_TYPE, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use common::errors::{ConfigError, DatabaseError, ServiceError, ValidationError};
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// RFC 9457 Problem Details payload.
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ApiError {
    /// A URI reference that identifies the problem type.
    pub r#type: String,
    /// A short, human-readable summary of the problem type.
    pub title: String,
    /// HTTP status code for this occurrence of the problem.
    pub status: u16,
    /// A human-readable explanation specific to this occurrence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// A URI reference that identifies the specific occurrence.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance: Option<String>,
    /// Optional machine-friendly extension value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

impl ApiError {
    pub fn new(
        problem_type: impl Into<String>,
        title: impl Into<String>,
        status: StatusCode,
    ) -> Self {
        Self {
            r#type: problem_type.into(),
            title: title.into(),
            status: status.as_u16(),
            detail: None,
            instance: None,
            code: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }
}

/// API error response wrapper with status code.
#[derive(Debug)]
pub struct ApiErrorResponse {
    pub status: StatusCode,
    pub error: ApiError,
}

impl ApiErrorResponse {
    pub fn new(
        status: StatusCode,
        problem_type: impl Into<String>,
        title: impl Into<String>,
    ) -> Self {
        Self {
            status,
            error: ApiError::new(problem_type, title, status),
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.error = self.error.with_detail(detail);
        self
    }

    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.error = self.error.with_code(code);
        self
    }
}

impl IntoResponse for ApiErrorResponse {
    fn into_response(self) -> Response {
        let mut response = (self.status, Json(self.error)).into_response();
        response.headers_mut().insert(
            CONTENT_TYPE,
            HeaderValue::from_static("application/problem+json"),
        );
        response
    }
}

// Convenience constructors for common errors.
impl ApiErrorResponse {
    pub fn internal_server_error(title: impl Into<String>) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "https://data-commons.dev/problems/internal-error",
            title,
        )
        .with_code("INTERNAL_ERROR")
    }

    pub fn service_unavailable(title: impl Into<String>) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "https://data-commons.dev/problems/service-unavailable",
            title,
        )
        .with_code("SERVICE_UNAVAILABLE")
    }

    pub fn bad_request(title: impl Into<String>) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            "https://data-commons.dev/problems/bad-request",
            title,
        )
        .with_code("BAD_REQUEST")
    }

    pub fn not_found(title: impl Into<String>) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "https://data-commons.dev/problems/not-found",
            title,
        )
        .with_code("NOT_FOUND")
    }

    pub fn unauthorized(title: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "https://data-commons.dev/problems/unauthorized",
            title,
        )
        .with_code("UNAUTHORIZED")
    }

    pub fn forbidden(title: impl Into<String>) -> Self {
        Self::new(
            StatusCode::FORBIDDEN,
            "https://data-commons.dev/problems/forbidden",
            title,
        )
        .with_code("FORBIDDEN")
    }

    pub fn unprocessable_entity(title: impl Into<String>) -> Self {
        Self::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "https://data-commons.dev/problems/validation-error",
            title,
        )
        .with_code("VALIDATION_ERROR")
    }
}

// Convert shared error types to API responses.
impl From<DatabaseError> for ApiErrorResponse {
    fn from(err: DatabaseError) -> Self {
        match err {
            DatabaseError::EntityNotFound { entity_type, id } => {
                Self::not_found(format!("{} with id {} not found", entity_type, id))
            }
            DatabaseError::ConnectionFailed { .. } => {
                Self::service_unavailable("Database connection failed").with_detail(err.to_string())
            }
            DatabaseError::QueryFailed { .. } => {
                Self::internal_server_error("Database query failed").with_detail(err.to_string())
            }
            DatabaseError::TransactionFailed { .. } => {
                Self::internal_server_error("Database transaction failed")
                    .with_detail(err.to_string())
            }
            DatabaseError::MigrationFailed { .. } => {
                Self::internal_server_error("Database migration failed")
                    .with_detail(err.to_string())
            }
        }
    }
}

impl From<ValidationError> for ApiErrorResponse {
    fn from(err: ValidationError) -> Self {
        match err {
            ValidationError::RequiredField { .. } | ValidationError::InvalidField { .. } => {
                Self::unprocessable_entity("Validation failed").with_detail(err.to_string())
            }
        }
    }
}

impl From<ConfigError> for ApiErrorResponse {
    fn from(err: ConfigError) -> Self {
        Self::internal_server_error("Configuration error").with_detail(err.to_string())
    }
}

impl From<ServiceError> for ApiErrorResponse {
    fn from(err: ServiceError) -> Self {
        match err {
            ServiceError::ServiceUnavailable { .. } => {
                Self::service_unavailable("Dependent service unavailable")
                    .with_detail(err.to_string())
            }
            ServiceError::RequestTimeout { .. } => Self::new(
                StatusCode::REQUEST_TIMEOUT,
                "https://data-commons.dev/problems/request-timeout",
                "Request timed out",
            )
            .with_code("REQUEST_TIMEOUT")
            .with_detail(err.to_string()),
            ServiceError::InvalidResponse { .. } => {
                Self::internal_server_error("Invalid response from dependent service")
                    .with_detail(err.to_string())
            }
            ServiceError::AuthenticationFailed => Self::unauthorized("Authentication failed"),
            ServiceError::AuthorizationFailed => Self::forbidden("Insufficient permissions"),
        }
    }
}

impl From<anyhow::Error> for ApiErrorResponse {
    fn from(err: anyhow::Error) -> Self {
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

        Self::internal_server_error("Internal server error").with_detail(err.to_string())
    }
}

impl From<sea_orm::DbErr> for ApiErrorResponse {
    fn from(err: sea_orm::DbErr) -> Self {
        ApiErrorResponse::internal_server_error("Database error").with_detail(err.to_string())
    }
}
