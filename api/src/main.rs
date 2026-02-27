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
use axum::{body::Body, http::Request as HttpRequest, Router};
use common::config::AppConfig;
use common::telemetry::{self, KeyValue, TelemetrySetup};
use sentry::integrations::tower::{NewSentryLayer, SentryHttpLayer};
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

fn main() -> Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Load API-specific configuration (type-safe)
    let config = AppConfig::load_api().context("Failed to load configuration")?;

    // Initialize telemetry (Sentry if configured, basic tracing otherwise)
    let telemetry_setup = TelemetrySetup::from_config(
        "api",
        config.telemetry(),
        &config.logging().level,
        config.logging().format.as_deref(),
    )
    .context("Failed to create telemetry setup")?;

    let _telemetry_guard =
        telemetry::init_telemetry(telemetry_setup).context("Failed to initialize telemetry")?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Failed to create Tokio runtime")?;

    if let Err(err) = runtime.block_on(run(config)) {
        sentry::integrations::anyhow::capture_anyhow(&err);
        return Err(err);
    }

    Ok(())
}

async fn run(config: AppConfig) -> Result<()> {
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

    // Add Sentry HTTP middleware only when Sentry is active.
    if config.telemetry().enabled && telemetry::is_sentry_enabled() {
        app = app
            // Axum applies layers in reverse order; keep this ordering to avoid leaks.
            .layer(SentryHttpLayer::new().enable_transaction())
            .layer(NewSentryLayer::<HttpRequest<Body>>::new_from_top())
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
