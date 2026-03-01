# Development Guide

This guide covers development workflows, best practices, and common tasks for the Data Commons platform.

## Development Environment Setup

### Prerequisites

- **Rust 1.89+** with Cargo
- **PostgreSQL/PlanetScale Postgres** database access
- **Git** for version control
- **direnv + Doppler CLI** for environment loading

### Initial Setup

1. **Clone and setup project**
   ```bash
   git clone <repository-url>
   cd data-commons
   ```

2. **Install development tools**
   ```bash
   # Install SeaORM CLI used by entity generation workflow
   cargo install sea-orm-cli --version 1.1.15 --locked
   ```

3. **Configure environment**
   ```bash
   cp .env.example .env
   # Edit .env if needed, then load Doppler-managed environment
   direnv allow
   ```

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

# Or run in development mode with auto-reload (if configured)
cargo watch -x "run -p api"
```

## Project Structure

### Workspace Organization

```
data-commons/
├── api/                     # REST API server
│   ├── src/
│   │   ├── main.rs         # Application entry point
│   │   ├── handlers/       # HTTP request handlers
│   │   │   └── health.rs   # Health check endpoints
│   │   └── lib.rs          # Library exports
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
│       ├── src/
│       │   ├── mod.rs      # Entity exports
│       │   └── prelude.rs  # Common imports
│       └── Cargo.toml
├── migration/              # Database migrations
│   ├── src/
│   │   ├── lib.rs          # Migrator definition
│   │   ├── main.rs         # CLI entry point
│   │   └── m*.rs           # Individual migrations
│   └── Cargo.toml
└── .cargo/
    └── config.toml         # Cargo aliases
```

### Dependency Architecture

```
┌─────────────────────────────────────┐
│           Applications              │
│  ┌─────┐ ┌───────────┐ ┌─────────┐  │
│  │ API │ │ Indexer-1 │ │ Others  │  │
│  └─────┘ └───────────┘ └─────────┘  │
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
    let config = AppConfig::load()
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

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables first
    dotenvy::dotenv().ok();
    
    // Load hierarchical configuration
    let config = AppConfig::load()?;
    
    // Initialize tracing with configured level
    common::utils::init_tracing(&config.logging.level)?;
    
    // Use configuration
    let db = db_core::connection::from_config(&config.database).await?;
    
    Ok(())
}
```

## Adding New Features

### Adding New API Endpoints

1. **Create handler function**
   ```rust
   // api/src/handlers/users.rs
   use axum::{extract::State, Json};
   use utoipa::path;
   
   #[utoipa::path(
       get,
       path = "/api/users",
       responses(
           (status = 200, description = "List users successfully", body = [User]),
           (status = 500, description = "Internal server error")
       )
   )]
   pub async fn list_users(
       State(db): State<DatabaseConnection>,
   ) -> Result<Json<Vec<User>>, AppError> {
       let users = Users::find().all(&db).await?;
       Ok(Json(users))
   }
   ```

2. **Add to router**
   ```rust
   // api/src/main.rs
   let app = Router::new()
       .route("/api/users", get(handlers::users::list_users))
       .with_state(db);
   ```

3. **Update OpenAPI spec**
   ```rust
   #[derive(OpenApi)]
   #[openapi(
       paths(
           handlers::health::health_check,
           handlers::users::list_users,  // Add new endpoint
       ),
       components(
           schemas(HealthResponse, User)  // Add new schemas
       )
   )]
   struct ApiDoc;
   ```

### Adding New Indexer

See the detailed guide in the main README.md under "Adding New Indexers".

### Adding Database Changes

1. **Create migration**
   ```bash
   cargo migrate-generate add_user_preferences_table
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
   use entities::{user_preferences, prelude::*};
   ```

## Testing

The project uses a comprehensive testing strategy with testcontainers-rs for real database testing.

### Test Structure

Tests are organized to mirror the implementation structure for better maintainability:

```
api/tests/
├── common/                 # Shared test utilities
│   └── mod.rs             # TestDatabase, fixtures, tracing setup
├── handlers/              # Handler tests (business logic)
│   └── health/
│       ├── mod.rs
│       └── health_check.rs   # Handler-specific unit tests
├── routes/                # Route tests (HTTP behavior)
│   └── health/
│       └── mod.rs            # HTTP methods, headers, performance
└── integration.rs         # Main integration test entry point
```

### Test Categories

- **Handler Tests**: Focus on business logic, database interactions, validation
- **Route Tests**: Focus on HTTP behavior, status codes, headers, performance
- **Integration Tests**: End-to-end testing through the full request lifecycle

### Running Tests

```bash
# All tests across workspace
cargo test-all

# API tests specifically
cargo test -p api

# Specific test modules
cargo test -p api handlers::health
cargo test -p api routes::health

# Individual packages
cargo test -p discourse-indexer
cargo test -p near-indexer

# With output and timing
cargo test -p api -- --nocapture --test-threads=1
```

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
use rstest::*;
use crate::common::{init_test_tracing, TestDatabase};

#[fixture]
async fn test_db() -> TestDatabase {
    TestDatabase::new().await
}

#[rstest]
#[tokio::test]
async fn test_health_handler_with_database(#[future] test_db: TestDatabase) {
    init_test_tracing();
    
    let test_db = test_db.await;
    let server = create_test_server_with_db(Some(test_db.db())).await;

    let response = server.get("/health").await;
    response.assert_status_ok();

    let json: Value = response.json();
    assert_eq!(json["status"], "healthy");
    assert_eq!(json["database"]["connected"], true);
}
```

#### Route Tests (HTTP Behavior)

```rust
use axum::http::StatusCode;
use axum_test::TestServer;

#[rstest]
#[tokio::test]
async fn test_health_endpoint_http_methods() {
    let server = create_test_server_with_db(None).await;

    // GET should work
    let response = server.get("/health").await;
    response.assert_status_ok();

    // POST should not be allowed
    let response = server.post("/health").await;
    response.assert_status(StatusCode::METHOD_NOT_ALLOWED);
}
```

#### Test Database Setup

The `TestDatabase` struct manages PostgreSQL containers via testcontainers-rs:

```rust
pub struct TestDatabase {
    pub connection: DatabaseConnection,
    // Keep container alive for test duration
    pub _container: testcontainers::ContainerAsync<testcontainers_modules::mysql::Mysql>,
}

impl TestDatabase {
    pub async fn new() -> Self {
        // Creates PostgreSQL container, runs migrations, returns connection
    }
    
    pub fn db(&self) -> DatabaseConnection {
        self.connection.clone()
    }
}
```

## Debugging

### Logging Configuration

```bash
# Set environment variable for detailed logs
RUST_LOG=debug cargo run-api

# Or configure in config.toml
[logging]
level = "debug"
```

### Database Debugging

```bash
# Check connection
cargo migrate-status

# Inspect current schema (discourse)
sea-orm-cli generate entity --database-schema discourse -o /tmp/current_schema_discourse

# Inspect current schema (near)
sea-orm-cli generate entity --database-schema near -o /tmp/current_schema_near

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
cargo fmt -- --check

# Lint code (if clippy is configured)
cargo clippy -- -D warnings

# Check all workspace crates
cargo check-all
```

### Pre-commit Checks

Create `.git/hooks/pre-commit`:
```bash
#!/bin/sh
cargo fmt -- --check &&
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
RUST_LOG=info
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
cargo clean-all
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

1. **Check logs**: Enable debug logging with `RUST_LOG=debug`
2. **Verify configuration**: Check your `.env` and `config.toml` files
3. **Check database**: Ensure migrations are applied and database is accessible
4. **Clean rebuild**: Use `cargo clean-all && cargo check-all`

---

For more specific guides, see:
- [MIGRATIONS.md](./MIGRATIONS.md) - Database migration guide
- [API.md](./API.md) - API development guide
- Main [README.md](../README.md) - Project overview and quick start
