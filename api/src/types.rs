//! API request/response types and DTOs

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Health check response
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    /// Service status
    pub status: String,
    /// Current timestamp
    pub timestamp: String,
    /// Version information
    pub version: String,
    /// Database connectivity status
    pub database: DatabaseHealth,
}

/// Database health information
#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct DatabaseHealth {
    /// Whether database is accessible
    pub connected: bool,
    /// Database response time in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_time_ms: Option<u64>,
    /// Optional error message if not connected
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
