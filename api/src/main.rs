//! OpenAPI server with Axum and Utoipa
//!
//! This server provides a REST API with automatic OpenAPI documentation
//! and Swagger UI for interacting with the data commons platform.

mod docs;
mod errors;
mod handlers;
mod routes;
mod state;
mod types;

use anyhow::{Context, Result};
use axum::Router;
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use common::config::AppConfig;
use common::telemetry::{self, TelemetrySetup};
use opentelemetry::KeyValue;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Load API-specific configuration (type-safe)
    let config = AppConfig::load_api().context("Failed to load configuration")?;

    // Initialize telemetry (Dash0 if configured, basic tracing otherwise)
    let telemetry_setup =
        TelemetrySetup::from_config("api", config.telemetry(), &config.logging().level)
            .context("Failed to create telemetry setup")?;

    let _telemetry_guard =
        telemetry::init_telemetry(telemetry_setup).context("Failed to initialize telemetry")?;

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
    let app_state = state::AppState::from_database(db_result.ok());

    // Create the router with API routes
    let mut app = Router::new()
        .nest("/api/v1", routes::create_routes())
        .merge(routes::health::health_routes());

    // Add OpenTelemetry middleware only when telemetry is enabled and credentials are present
    if config.telemetry().enabled
        && std::env::var("DASH0_OTLP_ENDPOINT").is_ok()
        && std::env::var("DASH0_AUTH_TOKEN").is_ok()
    {
        app = app
            .layer(OtelInResponseLayer) // Add trace context to responses
            .layer(OtelAxumLayer::default()) // Create spans for requests
            .layer(TraceLayer::new_for_http()); // HTTP tracing
    }

    let app = app.with_state(app_state.clone());

    // Add Swagger UI with disabled try-it-out functionality
    let app = app.merge(
        SwaggerUi::new("/swagger-ui")
            .url("/api-docs/openapi.json", docs::ApiDoc::openapi())
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

    // Record server startup metric if telemetry is enabled
    if config.telemetry().enabled {
        telemetry::increment_counter(
            "server.startup",
            &[
                KeyValue::new("service", "api"),
                KeyValue::new("port", addr.port() as i64),
            ],
        );
    }

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind to address")?;

    axum::serve(listener, app.into_make_service())
        .await
        .context("Server failed to start")?;

    Ok(())
}
