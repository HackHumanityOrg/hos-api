//! Health check endpoint

use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{extract::State, Json};
use entities::sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use tracing::info;

/// Health check endpoint (with database connection)
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
            // Simple health check query
            match db
                .execute(Statement::from_string(
                    DatabaseBackend::MySql,
                    "SELECT 1".to_string(),
                ))
                .await
            {
                Ok(_) => {
                    info!("Database health check passed");
                    DatabaseHealth {
                        connected: true,
                        error: None,
                    }
                }
                Err(e) => {
                    tracing::error!("Database health check failed: {}", e);
                    DatabaseHealth {
                        connected: false,
                        error: Some(format!("Database connection error: {}", e)),
                    }
                }
            }
        }
        None => DatabaseHealth {
            connected: false,
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
