# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

### Essential Development Commands
```bash
# Verify everything works before committing
cargo check-all && cargo test-all

# Database operations
cargo migrate-up                # Apply pending migrations
cargo migrate-generate <name>   # Create new migration
cargo generate-entities        # Generate SeaORM entities from schema

# Run services
cargo run-api                   # Start API server (port 3000)
cargo run-indexer-1            # Start individual indexers
```

### Testing Commands
```bash
# Run all tests (includes real MySQL containers via testcontainers-rs)
cargo test-all

# Run specific test suites
cargo test -p api                    # API tests with database containers
cargo test -p api handlers::health  # Handler-specific tests (business logic)
cargo test -p api routes::health    # Route-specific tests (HTTP behavior)

# Single test with output
cargo test -p api test_health_handler_with_database -- --nocapture
```

### Database Management
```bash
# Migration workflow
cargo migrate-status    # Check current status
cargo migrate-fresh     # Reset and reapply all migrations
cargo migrate-reset     # Rollback all migrations
cargo migrate-down      # Rollback last migration
```

## Architecture

### Monorepo Structure
- **Cargo Workspace** with dependency inheritance across all crates
- **Shared Libraries**: `common` (config/errors), `db-core` (connections), `entities` (SeaORM models)
- **API Server**: Axum + Utoipa for OpenAPI/Swagger documentation
- **Indexers**: Three parallel data processing services with database access
- **Testing**: Structured to mirror implementation (handlers/ vs routes/ separation)

### Key Design Patterns

#### Error Handling Strategy
- **Libraries** (`shared/*`): Use `thiserror` for structured error types
- **Applications** (`api`, `indexers/*`): Use `anyhow` for flexible error handling
- All database connections are optional - services gracefully degrade without DB

#### Database Architecture
- **SeaORM** with MySQL/PlanetScale (SSL/TLS enabled)
- **Connection sharing**: `AppState` holds optional `DatabaseConnection`
- **Configurable parameters**: All SeaORM settings in `config.toml` (timeouts, pool size, etc.)
- **Migration-first**: Schema changes via SeaORM migrations, then `cargo generate-entities`

#### Configuration System
- **Hierarchical loading** via Figment: Environment variables → config.toml → defaults
- **Secrets via ENV**: `DATABASE_URL` must be set in `.env` file
- **Non-secrets in TOML**: Connection pools, timeouts, server settings in `config.toml`

#### Testing Strategy
- **Real Database Testing**: testcontainers-rs spins up MySQL containers
- **Mirrored Structure**: `api/tests/handlers/` mirrors `api/src/handlers/`
- **Separated Concerns**: Handler tests focus on business logic, route tests on HTTP behavior
- **Fixture-based**: rstest fixtures for database setup and cleanup

### Important Implementation Details

#### Database Connection Pattern
```rust
// All services use this pattern for optional database access
let db_result = db_core::connection::from_config(&config.database).await;
match &db_result {
    Ok(_) => info!("Database connected"),
    Err(e) => info!("Starting without database: {}", e), // Graceful degradation
}
let app_state = AppState::new(db_result.ok());
```

#### Test Database Pattern
```rust
// TestDatabase manages container lifecycle automatically
#[fixture]
async fn test_db() -> TestDatabase {
    TestDatabase::new().await  // Starts container + runs migrations
}
// Container stays alive due to _container field until test completes
```

#### Observability & Telemetry Architecture
- **Dash0 Integration**: Complete OpenTelemetry integration for logs, traces, and metrics
- **Centralized telemetry module**: `shared/common/src/telemetry.rs` handles all OTLP exports
- **HTTP-based exports**: Uses JSON over HTTPS to Dash0 endpoints (`/v1/traces`, `/v1/metrics`, `/v1/logs`)
- **Graceful degradation**: Services start without telemetry if Dash0 credentials unavailable
- **Automatic instrumentation**: HTTP requests, database queries, and custom business logic
- **Export timing**: Traces (1s), Metrics (2s), Logs (real-time) with configurable batching

#### Project-Scoped Tools
- **cargo-run-bin** manages `sea-orm-cli` as project dependency
- **Cargo aliases** in `.cargo/config.toml` provide shortcuts
- Never install global tools - everything is workspace-scoped

### Adding New Components

#### New Indexer Service
1. Create `indexers/indexer-N/` with standard structure
2. Add to workspace members in root `Cargo.toml`
3. Add `run-indexer-N` alias to `.cargo/config.toml`
4. Follow existing pattern: config loading → telemetry init → database connection
5. Add telemetry configuration to `config.toml` with appropriate `service.component` attribute

#### New API Endpoint
1. Create handler in `api/src/handlers/` with Utoipa annotations
2. Add to router in `api/src/routes/`
3. Update OpenAPI struct in `api/src/main.rs`
4. Create tests in `api/tests/handlers/` and `api/tests/routes/`

#### New Database Table
1. `cargo migrate-generate create_table_name`
2. Edit generated migration file
3. `cargo migrate-up`
4. `cargo generate-entities` to update Rust models

#### Telemetry Usage Patterns
```rust
// Initialize telemetry in main.rs (after config loading)
let telemetry_setup = common::telemetry::TelemetrySetup::from_config(
    "service-name",
    &config.telemetry,
    &config.logging.level,
)?;
let _telemetry_guard = common::telemetry::init_telemetry(telemetry_setup)?;

// Custom metrics in handlers
common::telemetry::increment_counter("api.requests", &[
    KeyValue::new("endpoint", "/health"),
    KeyValue::new("method", "GET"),
]);

let histogram = common::telemetry::duration_histogram("api.request_duration");
histogram.record(duration_ms, &[KeyValue::new("endpoint", "/health")]);

// Tracing spans (automatic via axum-tracing-opentelemetry + manual)
#[tracing::instrument]
async fn my_handler() {
    // Automatic span creation and HTTP request tracing
    tracing::info!("Processing request");  // Correlated with traces
}
```

## Configuration Requirements

### Environment Variables (.env)
```bash
DATABASE_URL=mysql://user:pass@host/db  # Required for all services

# Dash0 OpenTelemetry Integration (optional - services gracefully degrade without)
DASH0_OTLP_ENDPOINT=https://ingress.region.aws.dash0.com  # Your Dash0 OTLP endpoint
DASH0_AUTH_TOKEN=auth_your_token_here                     # Your Dash0 authentication token
```

### config.toml Structure
All SeaORM connection parameters, server settings, and logging configuration must be in config.toml, not hardcoded.

#### Telemetry Configuration
Each service includes a `[telemetry]` section for Dash0 OpenTelemetry integration:

```toml
[telemetry]
# Dash0 OpenTelemetry configuration
# OTLP endpoint and auth token must be set via environment variables:
# - DASH0_OTLP_ENDPOINT (e.g., "https://ingress.us-west-2.aws.dash0.com")
# - DASH0_AUTH_TOKEN
enabled = true
service_version = "0.1.0"
environment = "development"    # Options: development, staging, production
enable_metrics = true
enable_traces = true
enable_logs = true
batch_size = 512               # Max number of spans/logs/metrics per batch
export_timeout_sec = 10        # Timeout for export operations
export_interval_sec = 5        # How often to export batches

# Service-specific resource attributes
[telemetry.resource_attributes]
"service.namespace" = "data-commons"
"service.component" = "api"    # or "indexer-1", "indexer-2", etc.
"deployment.region" = "local"
```

### Swagger UI
- Automatically available at `/swagger-ui` when API server runs
- OpenAPI spec at `/api-docs/openapi.json`
- Try-it-out functionality can be disabled via utoipa-swagger-ui Config if needed
