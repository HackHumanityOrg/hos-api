//! Health check endpoint

use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{extract::State, Json};
use entities::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::time::Instant;
use tracing::info;

/// Health check endpoint
#[utoipa::path(
    get,
    path = "/health",
    tag = "health",
    responses(
        (status = 200, description = "Health check passed", body = HealthResponse),
        (status = 500, description = "Health check failed", body = crate::errors::ApiError)
    )
)]
pub async fn health_check(
    State(app_state): State<AppState>,
) -> Result<Json<HealthResponse>, ApiErrorResponse> {
    info!("Health check requested");

    // Test database connection if available
    let database_health = match &app_state.db {
        Some(db) => {
            // Measure database response time
            let start_time = Instant::now();

            // Simple health check query
            match db
                .execute(Statement::from_string(
                    DatabaseBackend::MySql,
                    "SELECT 1".to_string(),
                ))
                .await
            {
                Ok(_) => {
                    let response_time = start_time.elapsed().as_millis() as u64;
                    info!("Database health check passed in {}ms", response_time);
                    DatabaseHealth {
                        connected: true,
                        response_time_ms: Some(response_time),
                        error: None,
                    }
                }
                Err(e) => {
                    let response_time = start_time.elapsed().as_millis() as u64;
                    tracing::error!("Database health check failed in {}ms: {}", response_time, e);
                    DatabaseHealth {
                        connected: false,
                        response_time_ms: Some(response_time),
                        error: Some(format!("Database connection error: {}", e)),
                    }
                }
            }
        }
        None => DatabaseHealth {
            connected: false,
            response_time_ms: None,
            error: Some("Database connection not configured".to_string()),
        },
    };

    let response = HealthResponse {
        status: if database_health.connected {
            "healthy".to_string()
        } else {
            "degraded".to_string()
        },
        timestamp: chrono::Utc::now().to_rfc3339(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        database: database_health,
    };

    Ok(Json(response))
}
