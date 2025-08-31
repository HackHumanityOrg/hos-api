//! OpenAPI server with Axum and Utoipa
//!
//! This server provides a REST API with automatic OpenAPI documentation
//! and Swagger UI for interacting with the data commons platform.

mod errors;
mod handlers;
mod routes;
mod state;
mod types;

use anyhow::{Context, Result};
use axum::Router;
use common::config::AppConfig;
use std::net::SocketAddr;
use tracing::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Data Commons API",
        version = "0.1.0",
        description = "REST API for the Data Commons platform",
        license(name = "MIT", url = "https://opensource.org/licenses/MIT")
    ),
    paths(
        handlers::health::health_check,
    ),
    components(schemas(
        types::HealthResponse,
        types::DatabaseHealth,
        errors::ApiError,
    )),
    tags(
        (name = "health", description = "Health check endpoints")
    )
)]
struct ApiDoc;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Load API-specific configuration (type-safe)
    let config = AppConfig::load_api().context("Failed to load configuration")?;

    // Initialize tracing
    common::utils::init_tracing(&config.logging().level).context("Failed to initialize tracing")?;

    info!("Starting API Server...");

    // Try to initialize database connection from configuration
    let db_result = db_core::connection::from_config(config.database()).await;

    match &db_result {
        Ok(_) => {
            info!("Database connection established successfully");
        }
        Err(e) => {
            tracing::warn!("Failed to connect to database: {}", e);
            info!("Starting server without database connection - health check will show degraded status");
        }
    }

    // Create application state
    let app_state = state::AppState::new(db_result.ok());

    // Create the router with API routes
    let app = Router::new()
        .nest("/api/v1", routes::create_routes())
        .merge(routes::health::health_routes())
        .with_state(app_state.clone());

    // Add Swagger UI with disabled try-it-out functionality
    let app = app.merge(
        SwaggerUi::new("/swagger-ui")
            .url("/api-docs/openapi.json", ApiDoc::openapi())
            .config(
                utoipa_swagger_ui::Config::new(["/api-docs/openapi.json"])
                    .display_request_duration(true),
            ),
    );

    let server_config = config.server_required();
    let addr = SocketAddr::new(
        server_config.host.parse().context("Invalid server host")?,
        server_config.port,
    );

    info!("Server starting on http://{}", addr);
    info!("Swagger UI available at http://{}/swagger-ui", addr);
    info!(
        "OpenAPI spec available at http://{}/api-docs/openapi.json",
        addr
    );

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind to address")?;

    axum::serve(listener, app.into_make_service())
        .await
        .context("Server failed to start")?;

    Ok(())
}
