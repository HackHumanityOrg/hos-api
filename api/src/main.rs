//! OpenAPI server with Axum and Utoipa
//!
//! This server provides a REST API with automatic OpenAPI documentation
//! and Swagger UI for interacting with the HOS API platform.

mod docs;
mod errors;
mod handlers;
mod routes;
mod state;
mod types;

use anyhow::{Context, Result};
use axum::Router;
use common::config::AppConfig;
use common::telemetry::{self, KeyValue, TelemetrySetup};
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{IpAddr, Ipv6Addr, SocketAddr};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;
use tracing::{error, info};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

fn main() -> Result<()> {
    // Load environment variables
    dotenvy::dotenv().ok();

    // Load API-specific configuration (type-safe)
    let config = AppConfig::load_api().context("Failed to load configuration")?;

    // Initialize telemetry and tracing
    let telemetry_setup =
        TelemetrySetup::from_config("api", config.telemetry(), &config.logging().level)
            .context("Failed to create telemetry setup")?;

    let _telemetry_guard =
        telemetry::init_telemetry(telemetry_setup).context("Failed to initialize telemetry")?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("Failed to create Tokio runtime")?;

    if let Err(err) = runtime.block_on(run(config)) {
        error!(error = ?err, "API service terminated with error");
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
    let app = Router::new()
        .nest("/api/v1", routes::create_routes())
        .nest("/api/agora", routes::agora::agora_routes())
        .merge(routes::health::health_routes())
        .layer(TraceLayer::new_for_http());

    let app = app.with_state(app_state.clone());

    // Add Swagger UI with docs exploration enabled.
    let app = app.merge(
        SwaggerUi::new("/swagger-ui")
            .url("/api-docs/openapi.json", docs::ApiDoc::openapi())
            .config(
                utoipa_swagger_ui::Config::new(["/api-docs/openapi.json"])
                    .display_request_duration(true)
                    .display_operation_id(true)
                    .filter(true)
                    .doc_expansion("list"),
            ),
    );

    let server_config = config.server_required();
    let host = server_config.host.parse().context("Invalid server host")?;
    let listener = bind_listener(host, server_config.port)?;
    let bound_addr = listener
        .local_addr()
        .context("Failed to resolve bound listener address")?;

    info!("Server starting on http://{}", bound_addr);
    info!("Swagger UI available at http://{}/swagger-ui", bound_addr);
    info!(
        "OpenAPI spec available at http://{}/api-docs/openapi.json",
        bound_addr
    );

    // Record server startup metric if telemetry is enabled
    if config.telemetry().enabled {
        telemetry::increment_counter(
            "server.startup",
            &[
                KeyValue::new("service", "api"),
                KeyValue::new("port", bound_addr.port() as i64),
            ],
        );
    }

    axum::serve(listener, app.into_make_service())
        .await
        .context("Server failed to start")?;

    Ok(())
}

fn bind_listener(host: IpAddr, port: u16) -> Result<TcpListener> {
    if host.is_unspecified() {
        match bind_dual_stack_listener(port) {
            Ok(listener) => {
                info!(
                    requested_addr = %SocketAddr::new(host, port),
                    bound_addr = %listener
                        .local_addr()
                        .context("Failed to resolve dual-stack listener address")?,
                    "Bound dual-stack listener for public and private Railway traffic"
                );
                return Ok(listener);
            }
            Err(error) => {
                tracing::warn!(
                    requested_addr = %SocketAddr::new(host, port),
                    error = ?error,
                    "Falling back to single-stack listener after dual-stack bind failed"
                );
            }
        }
    }

    let std_listener = std::net::TcpListener::bind(SocketAddr::new(host, port))
        .context("Failed to bind single-stack listener")?;
    std_listener
        .set_nonblocking(true)
        .context("Failed to configure non-blocking listener")?;

    TcpListener::from_std(std_listener).context("Failed to create Tokio listener from socket")
}

fn bind_dual_stack_listener(port: u16) -> Result<TcpListener> {
    let socket = Socket::new(Domain::IPV6, Type::STREAM, Some(Protocol::TCP))
        .context("Failed to create IPv6 TCP socket")?;
    socket
        .set_only_v6(false)
        .context("Failed to enable dual-stack listener")?;
    socket
        .set_nonblocking(true)
        .context("Failed to configure dual-stack socket as non-blocking")?;
    socket
        .bind(&SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), port).into())
        .context("Failed to bind dual-stack listener")?;
    socket
        .listen(1024)
        .context("Failed to listen on dual-stack socket")?;

    let std_listener: std::net::TcpListener = socket.into();
    TcpListener::from_std(std_listener)
        .context("Failed to create Tokio listener from dual-stack socket")
}
