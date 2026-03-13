# HOS API

A high-performance Rust monorepo for data indexing and API services built with modern async architecture.

## Architecture

- **API Server** - REST API with OpenAPI/Swagger documentation
- **Indexers** - Parallel data processing services  
- **Shared Libraries** - Database, configuration, utilities, and telemetry
- **Migrations** - SeaORM-powered PostgreSQL/PlanetScale Postgres schema management
- **Observability** - Structured JSON logging and tracing

```
hos-api/
├── api/                    # REST API server with Swagger UI
├── indexers/              # Data processing services (discourse, near, telegram)
├── shared/                # Shared libraries (common, db-core, entities)
├── migration/             # Database migrations
└── .cargo/               # Cargo aliases and configuration
```

## Quick Start

```bash
git clone <repository-url> && cd hos-api
cargo install sea-orm-cli --version 1.1.15 --locked
cp .env.example .env  # Edit with database settings
direnv allow
cargo migrate-up && cargo generate-entities
cargo run-api
```

**Access**: API at http://localhost:3000 • [Swagger UI](http://localhost:3000/swagger-ui) • [OpenAPI spec](http://localhost:3000/api-docs/openapi.json)

**Detailed setup**: [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)

## Database Operations

```bash
# Common operations
cargo migrate-up               # Apply migrations
cargo migrate-generate <name>  # Create new migration
cargo generate-entities       # Generate entities from schema
```

**Database guide**: [docs/MIGRATIONS.md](docs/MIGRATIONS.md)

## Development

```bash
# Common commands
cargo check-all && cargo test-all    # Verify everything works
cargo run-api                        # Start API server
cargo run-indexer-1                  # Start discourse indexer
cargo run-indexer-2                  # Start NEAR indexer
cargo run-indexer-3                  # Start Telegram indexer
```

**Development guide**: [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)
**API development**: [docs/API.md](docs/API.md)

## Railway Deployment

This repository is deployed as a monorepo with one Railway service per component.

- `api` service uses `/api/railway.toml`
- `discourse-indexer` service uses `/indexers/discourse-indexer/railway.toml`
- `near-indexer` service uses `/indexers/near-indexer/railway.toml`
- `telegram-indexer` service uses `/indexers/telegram-indexer/railway.toml`

Each service should be linked to the same GitHub repository (`HackHumanityOrg/hos-api`) on `main` with autodeploy enabled. Watch patterns are defined in each service config file to avoid cross-service rebuilds.

## Testing

```bash
cargo test-all              # Run all tests
cargo test -p api           # API tests with real database containers
```

**Features**: Real PostgreSQL containers • Handler/Route separation • Security testing • Performance validation

**Testing guide**: [docs/DEVELOPMENT.md#testing](docs/DEVELOPMENT.md#testing)

## Observability

The platform uses `tracing` with `tracing-subscriber` and emits structured JSON logs by default.

**Features**:
- **Logs**: Structured JSON logs for all services
- **Traces**: Span-aware request and background-job tracing
- **Filtering**: Log level control via `RUST_LOG`

## Tech Stack

**Backend**: Rust • Tokio • Axum • SeaORM • PostgreSQL/PlanetScale Postgres
**Observability**: tracing • structured JSON logging
**Testing**: testcontainers-rs • rstest • axum-test
**Docs**: Utoipa • Swagger UI
**Config**: Figment • Environment variables

## Contributing

```bash
cargo check-all && cargo test-all  # Verify your changes
```

**Contributing guide**: [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)
