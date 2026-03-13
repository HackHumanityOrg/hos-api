# Development Guide

This guide covers development workflows, best practices, and common tasks for the HOS API platform.

## Development Environment Setup

### Prerequisites

- **Rust stable** with Cargo
  The repo currently builds with Rust 1.86 locally, and the Dockerfiles use Rust 1.88.
- **Node.js 20+** with `pnpm`
- **PostgreSQL-compatible** database access
- **Git** for version control
- Optional: **direnv** for automatic env loading
- **Doppler CLI** if you want to use the checked-in `.envrc` workflow

### Initial Setup

1. **Clone and setup project**
   ```bash
   git clone <repository-url>
   cd hos-api
   ```

2. **Install development tools**
   ```bash
   # Install SeaORM CLI used by entity generation workflow
   cargo install sea-orm-cli --version 1.1.15 --locked
   ```

3. **Configure environment**
   ```bash
   cp .env.example .env.local
   # Edit .env.local, then load it through direnv
   direnv allow
   ```

   The checked-in `.envrc` runs `doppler secrets download` before sourcing
   `.env.local`, so `direnv allow` requires the Doppler CLI. If you are not
   using Doppler, skip `direnv allow` and export `DATABASE_URL` manually in
   your shell.

   DB-backed commands such as `cargo migrate-up`, `cargo migrate-down`,
   `cargo migrate-status`, `cargo migrate-reset`, `cargo migrate-fresh`,
   `cargo generate-entities`, and `cargo backfill-identity-accounts` require
   `DATABASE_URL` in the real shell environment. `cargo migrate-generate` does
   not.

4. **Initialize database**
   ```bash
   cargo migrate-up
   cargo generate-entities
   ```

5. **Verify setup**
   ```bash
   cargo check-all
   cargo test-all
   ```

## Development Workflow

### Daily Development

```bash
# Start your work session
cargo check-all              # Ensure everything compiles
cargo migrate-status         # Check for pending migrations

# After making changes
cargo check-all              # Check compilation
cargo test-all               # Run tests
cargo generate-entities      # Regenerate entities if schema changed
```

### Testing Changes

```bash
# Run specific service
cargo run-api                # Test API changes
cargo run-indexer-1          # Test discourse indexer changes
cargo run-indexer-2          # Test NEAR indexer changes
cargo run-indexer-3          # Test Telegram indexer changes

# Run the Next.js veNEAR explorer
cd web
cp .env.example .env.local   # Optional if you need to override the default local API URL
pnpm install
pnpm dev

# Or run in development mode with auto-reload (if configured)
cargo watch -x "run -p api"
```

The frontend expects the veNEAR API at `VENEAR_API_BASE_URL`. By default it points at `http://127.0.0.1:3000/api/v1/venear`, so the common local workflow is:

1. `cargo run-api`
2. `cd web && pnpm dev`

## Project Structure

### Workspace Organization

```
hos-api/
├── api/                     # REST API server
│   ├── src/
│   │   ├── main.rs         # Application entry point
│   │   ├── lib.rs          # Library exports used by tests
│   │   ├── docs.rs         # OpenAPI registration
│   │   ├── state.rs        # AppState with optional DB handle
│   │   ├── types.rs        # Shared DTOs and query structs
│   │   ├── handlers/       # Domain handlers
│   │   └── routes/         # Route wiring per domain
│   ├── config.toml         # API defaults overridden by env
│   ├── tests/              # Unit/e2e coverage plus support helpers
│   └── Cargo.toml
├── indexers/               # Data processing services
│   ├── discourse-indexer/  # Discourse data processor
│   ├── near-indexer/       # NEAR blockchain processor
│   └── telegram-indexer/   # Telegram channel listener
├── shared/                 # Shared libraries
│   ├── common/             # Common utilities
│   │   ├── src/
│   │   │   ├── config.rs   # Configuration management
│   │   │   ├── errors.rs   # Error type definitions
│   │   │   └── utils.rs    # Utility functions
│   │   └── Cargo.toml
│   ├── db-core/            # Database operations
│   │   ├── src/
│   │   │   ├── connection.rs # DB connection management
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   └── entities/           # SeaORM generated entities
│       ├── src/discourse/  # Discourse schema entities
│       ├── src/identity/   # Identity schema entities
│       ├── src/near/       # NEAR schema entities
│       ├── src/telegram/   # Telegram schema entities
│       ├── src/lib.rs      # Entity exports
│       ├── src/prelude.rs  # Cross-schema re-exports
│       └── Cargo.toml
├── migration/              # Database migrations
│   ├── src/
│   │   ├── lib.rs          # Migrator definition
│   │   ├── main.rs         # CLI entry point
│   │   ├── bin/            # Helper binaries (entity generation, backfills)
│   │   └── m*.rs           # Individual migrations
│   └── Cargo.toml
├── web/                    # Next.js veNEAR explorer
│   ├── src/app/            # App Router pages and layouts
│   ├── src/components/     # shadcn/ui and veNEAR-specific UI pieces
│   └── package.json        # Frontend scripts and dependencies
├── docs/                   # Architecture, API, and workflow guides
└── .cargo/
    └── config.toml         # Cargo aliases
```

### Dependency Architecture

```
┌─────────────────────────────────────┐
│           Applications              │
│  ┌─────┐ ┌───────────┐ ┌───────┐ ┌──────────┐ │
│  │ API │ │ discourse │ │ near  │ │ telegram │ │
│  └─────┘ └───────────┘ └───────┘ └──────────┘ │
└─────────────────┬───────────────────┘
                  │
┌─────────────────▼───────────────────┐
│          Shared Libraries           │
│  ┌────────┐ ┌─────────┐ ┌─────────┐ │
│  │Common  │ │DB-Core  │ │Entities │ │
│  └────────┘ └─────────┘ └─────────┘ │
└─────────────────────────────────────┘
```

## Coding Conventions

### Error Handling Strategy

- **Libraries** (`shared/*`) use `thiserror` for structured errors
- **Applications** (`api`, `indexer-*`) use `anyhow` for flexible error handling

```rust
// Library error (shared/common/src/errors.rs)
#[derive(Error, Debug, Clone)]
pub enum DatabaseError {
    #[error("Connection failed: {message}")]
    ConnectionFailed { message: String },
}

// Application error handling (api/src/main.rs)
use anyhow::{Context, Result};

async fn main() -> Result<()> {
    let config = AppConfig::load_api()
        .context("Failed to load configuration")?;
    Ok(())
}
```

### Async Patterns

```rust
// Prefer explicit async/await
async fn process_data() -> Result<Data> {
    let raw = fetch_data().await?;
    let processed = transform_data(raw).await?;
    Ok(processed)
}

// Use tokio::spawn for concurrent work
let handles: Vec<_> = items
    .into_iter()
    .map(|item| tokio::spawn(process_item(item)))
    .collect();

let results: Vec<_> = futures::future::join_all(handles).await;
```

### Configuration Management

```rust
use common::config::AppConfig;
use common::telemetry::{self, TelemetrySetup};

fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();

    let config = AppConfig::load_api()?;
    let telemetry_setup =
        TelemetrySetup::from_config("api", config.telemetry(), &config.logging().level)?;
    let _telemetry_guard = telemetry::init_telemetry(telemetry_setup)?;

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async move {
        let _db = db_core::connection::from_config(config.database()).await?;
        Ok(())
    })
}
```

## Adding New Features

### Adding New API Endpoints

1. **Create handler function**
   ```rust
   // api/src/handlers/discourse/mod.rs
   use axum::{
       extract::{Query, State},
       Json,
   };
   use utoipa::path;
   
   #[utoipa::path(
       get,
       path = "/api/v1/discourse/instances",
       params(CursorPaginationQuery),
       responses(
           (status = 200, description = "List indexed instances", body = InstancesListResponse),
           (status = 400, description = "Invalid request", body = crate::errors::ApiError),
           (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
       )
   )]
   pub async fn list_instances(
       State(app_state): State<AppState>,
       Query(query): Query<CursorPaginationQuery>,
   ) -> Result<Json<InstancesListResponse>, ApiErrorResponse> {
       let db = require_db(&app_state)?;
       let limit = resolve_limit(query.limit);
       // Build a SeaORM query, fetch `limit + 1` rows, and derive `next_cursor`.
       let _ = (db, limit);
       Ok(Json(InstancesListResponse {
           data: vec![],
           next_cursor: None,
           has_more: false,
       }))
   }
   ```

2. **Add to router**
   ```rust
   // api/src/routes/discourse/mod.rs
   pub fn discourse_routes() -> Router<AppState> {
       Router::new()
           .route("/discourse/instances", get(discourse::list_instances))
   }
   ```

3. **Update OpenAPI spec**
   ```rust
   #[derive(OpenApi)]
   #[openapi(
       paths(
           handlers::health::health_check,
           handlers::discourse::list_instances,
       ),
       components(
           schemas(HealthResponse, InstancesListResponse)
       )
   )]
   struct ApiDoc;
   ```

### Adding New Indexer

1. Add the crate under `indexers/<new-indexer>/` and register it in the
   workspace `Cargo.toml`.
2. Reuse `shared/common` for config loading and `shared/db-core` for database
   access instead of hand-rolling service setup.
3. Add a `config.toml` plus any deployable artifacts the service needs
   (`Dockerfile`, `railway.toml`, service README).
4. If the indexer writes new schema, add migrations first and regenerate
   entities before implementing ingestion logic.
5. Update `README.md`, `docs/ARCHITECTURE.md`, and any service-specific docs in
   the same change.

### Adding Database Changes

1. **Create migration**
   ```bash
   cargo migrate-generate add_identity_accounts_flags
   ```

2. **Implement migration**
   ```rust
   // Edit generated migration file
   async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
       manager.create_table(/* table definition */).await
   }
   ```

3. **Apply and generate entities**
   ```bash
   cargo migrate-up
   cargo generate-entities
   ```

4. **Update code to use new entities**
   ```rust
   use entities::identity::{accounts, prelude::Accounts};
   ```

## Testing

The project uses a comprehensive testing strategy with testcontainers-rs for real database testing.

### Test Structure

Tests are organized to mirror the implementation structure for better maintainability:

```
api/tests/
├── support/mod.rs         # Shared router and test database helpers
├── unit/                  # Deterministic unit tests for handlers/types/routes
├── e2e/                   # PostgreSQL-backed end-to-end tests
├── openapi.rs             # OpenAPI contract wrappers
├── routes.rs              # Route wiring wrappers
├── agora_live_db.rs       # Agora compatibility wrappers
└── venear_live_db.rs      # veNEAR canonical wrappers
```

### Test Categories

- **Handler Tests**: Focus on business logic, database interactions, validation
- **Route Tests**: Focus on HTTP behavior, status codes, headers, performance
- **E2E Tests**: End-to-end requests against disposable PostgreSQL instances

### Running Tests

```bash
# All tests across workspace
cargo test-all

# API tests specifically
cargo test -p api

# Focus on health-related API coverage
cargo test -p api health_

# OpenAPI contract smoke test
cargo test -p api openapi_spec_exposes_health_path_and_schemas

# Individual packages
cargo test -p discourse-indexer
cargo test -p near-indexer

# Rebuild checked-in NEAR fixture expectations
MODULE_KIND=raw ALL=true OVERWRITE_EXISTING=true \
  cargo run -p near-indexer --example generate_venear_fixture_expectations
MODULE_KIND=venear ALL=true OVERWRITE_EXISTING=true \
  cargo run -p near-indexer --example generate_venear_fixture_expectations

# With output and timing
cargo test -p api -- --nocapture --test-threads=1
```

For `near-indexer`, deterministic fixture expectations live under:

- `indexers/near-indexer/tests/fixtures/venear_governance/expectations/raw/`
- `indexers/near-indexer/tests/fixtures/venear_governance/expectations/venear/`

Each checked-in fixture, including logical range scenarios in
`indexers/near-indexer/tests/fixtures/venear_governance/ranges/`, must ship
with both a `raw` and a `venear` expectation file. The fixture tests now assert
exact canonical-table and materialized-view outputs rather than just checking
for non-empty ingestion.

#### Live HTTP Tests

The Discourse indexer test suite includes live upstream integration tests by default. These tests call real Discourse APIs (`meta.discourse.org` and `gov.near.org`) and persist results into PostgreSQL testcontainers.

- Run the full crate test suite:
  - `cargo test-discourse-indexer`

- Run with coverage enforcement (85% line coverage gate):
  - `./scripts/coverage_discourse_indexer.sh 85`

- Generate a full llvm-cov report (no gate):
  - `cargo coverage-discourse-indexer`

Notes:
- Live tests use bounded page limits, explicit timeouts, and one retry for transient upstream failures.
- Deterministic mocked tests still run alongside live tests to cover retry, caching, and edge-case logic without network dependence.

### Writing Tests

#### Handler Tests (Business Logic)

```rust
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

#[path = "../support/mod.rs"]
mod support;

#[tokio::test]
async fn health_route_is_wired_at_root() {
    support::init_test_tracing();

    let response = support::build_router_with_state(None)
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}
```

#### E2E Tests (HTTP + live PostgreSQL)

```rust
use api::types::HealthResponse;
use axum::{body::Body, http::Request};
use tower::ServiceExt;

#[path = "../support/mod.rs"]
mod support;

#[tokio::test]
async fn health_endpoint_reports_healthy_with_live_database() {
    support::init_test_tracing();

    let (router, _database) = support::router_with_database().await;
    let response = router
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .expect("response");

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    let payload: HealthResponse = serde_json::from_slice(&bytes).expect("health payload");

    assert_eq!(payload.status, "healthy");
    assert!(payload.database.connected);
}
```

#### Test Database Setup

The `TestDatabase` struct manages PostgreSQL containers via testcontainers-rs:

```rust
pub struct TestDatabase {
    pub connection: Arc<DatabaseConnection>,
    // Keep container alive for test duration
    pub _container: ContainerAsync<Postgres>,
}

impl TestDatabase {
    pub async fn new() -> Self {
        // Creates PostgreSQL container, runs migrations, returns connection
    }

    pub fn db(&self) -> Arc<DatabaseConnection> {
        self.connection.clone()
    }
}
```

## Debugging

### Logging Configuration

```bash
# Set environment variable for detailed logs
APP_LOGGING__LEVEL=debug cargo run-api

# Or configure in api/config.toml (or indexers/*/config.toml)
[logging]
level = "debug"
```

### Database Debugging

```bash
# Check connection
cargo migrate-status

# Inspect current schema (discourse)
sea-orm-cli generate entity --database-url "$DATABASE_URL" --database-schema discourse -o /tmp/current_schema_discourse

# Inspect current schema (near)
sea-orm-cli generate entity --database-url "$DATABASE_URL" --database-schema near -o /tmp/current_schema_near

# Reset database for testing
cargo migrate-reset
cargo migrate-up
```

### Performance Profiling

```bash
# Build with debug symbols
cargo build --release

# Use perf (Linux) or Instruments (macOS)
perf record --call-graph dwarf ./target/release/api

# Analyze
perf report
```

## Code Quality

### Formatting and Linting

```bash
# Format code
cargo fmt

# Check formatting
cargo fmt --check

# Lint the crate you changed
cargo clippy -p api --all-targets -- -D warnings

# Check all workspace crates
cargo check-all
```

### Pre-commit Checks

Create `.git/hooks/pre-commit`:
```bash
#!/bin/sh
cargo fmt --check &&
cargo check-all &&
cargo test-all --quiet
```

## Deployment Preparation

### Building for Production

```bash
# Build optimized binaries
cargo build --release

# Build specific service
cargo build --release -p api

# Check binary size
ls -la target/release/
```

### Environment Configuration

```bash
# Production environment variables
DATABASE_URL=postgresql://prod_user:password@prod_host:5432/database?sslmode=require
APP_LOGGING__LEVEL=info
# Use double underscores for nested keys
APP_SERVER__PORT=80
```

### Migration in Production

```bash
# Check migration status first
cargo migrate-status

# Apply pending migrations
cargo migrate-up

# Regenerate entities if needed
cargo generate-entities
```

## Troubleshooting

### Common Issues

**Compilation Errors:**
```bash
# Clean build artifacts
cargo clean
cargo check-all
```

**Database Connection Issues:**
```bash
# Verify DATABASE_URL
echo $DATABASE_URL

# Test connection
cargo migrate-status
```

**Entity Generation Issues:**
```bash
# Regenerate entities from the current database schema
cargo generate-entities
```

**Cargo Alias Issues:**
```bash
# Verify alias configuration
cat .cargo/config.toml

# Test specific alias
cargo migrate-status
```

### Getting Help

1. **Check logs**: Enable debug logging with `APP_LOGGING__LEVEL=debug`
2. **Verify configuration**: Check your `.env.local` or exported env vars,
   plus `api/config.toml` and `indexers/*/config.toml`. If you use `direnv`,
   make sure the Doppler CLI is installed because `.envrc` invokes it.
3. **Check database**: Ensure migrations are applied and database is accessible
4. **Clean rebuild**: Use `cargo clean && cargo check-all`

---

For more specific guides, see:
- [MIGRATIONS.md](./MIGRATIONS.md) - Database migration guide
- [API.md](./API.md) - API development guide
- Main [README.md](../README.md) - Project overview and quick start
