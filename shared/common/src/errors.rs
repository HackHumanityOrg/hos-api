//! Error types for shared library components using thiserror
//!
//! These structured error types are designed for library crates where
//! consumers need to match on specific error variants.

use thiserror::Error;

/// Database-related errors that can occur across the application
#[derive(Error, Debug, Clone)]
pub enum DatabaseError {
    #[error("Connection failed: {message}")]
    ConnectionFailed { message: String },

    #[error("Query execution failed: {message}")]
    QueryFailed { message: String },

    #[error("Transaction failed: {message}")]
    TransactionFailed { message: String },

    #[error("Migration failed: {message}")]
    MigrationFailed { message: String },

    #[error("Entity not found: {entity_type} with id {id}")]
    EntityNotFound { entity_type: String, id: String },
}

/// Validation errors for input data
#[derive(Error, Debug, Clone)]
pub enum ValidationError {
    #[error("Field '{field}' is required")]
    RequiredField { field: String },

    #[error("Field '{field}' has invalid value: {value}")]
    InvalidField { field: String, value: String },
}

/// Configuration-related errors
#[derive(Error, Debug, Clone)]
pub enum ConfigError {
    #[error("Missing environment variable: {var_name}")]
    MissingEnvVar { var_name: String },

    #[error("Invalid configuration value for {key}: {value}")]
    InvalidValue { key: String, value: String },

    #[error("Configuration file not found: {path}")]
    FileNotFound { path: String },

    #[error("Failed to parse configuration: {message}")]
    ParseError { message: String },
}

/// Service communication errors
#[derive(Error, Debug, Clone)]
pub enum ServiceError {
    #[error("Service unavailable: {service_name}")]
    ServiceUnavailable { service_name: String },

    #[error("Request timeout after {timeout_ms}ms")]
    RequestTimeout { timeout_ms: u64 },

    #[error("Invalid response from service: {details}")]
    InvalidResponse { details: String },

    #[error("Authentication failed")]
    AuthenticationFailed,

    #[error("Authorization failed: insufficient permissions")]
    AuthorizationFailed,
}
