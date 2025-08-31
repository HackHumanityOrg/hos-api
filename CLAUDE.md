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

#### Project-Scoped Tools
- **cargo-run-bin** manages `sea-orm-cli` as project dependency
- **Cargo aliases** in `.cargo/config.toml` provide shortcuts
- Never install global tools - everything is workspace-scoped

### Adding New Components

#### New Indexer Service
1. Create `indexers/indexer-N/` with standard structure
2. Add to workspace members in root `Cargo.toml`
3. Add `run-indexer-N` alias to `.cargo/config.toml`
4. Follow existing pattern: config loading → tracing init → database connection

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

## Configuration Requirements

### Environment Variables (.env)
```bash
DATABASE_URL=mysql://user:pass@host/db  # Required for all services
```

### config.toml Structure
All SeaORM connection parameters, server settings, and logging configuration must be in config.toml, not hardcoded.

### Swagger UI
- Automatically available at `/swagger-ui` when API server runs
- OpenAPI spec at `/api-docs/openapi.json`
- Try-it-out functionality can be disabled via utoipa-swagger-ui Config if needed