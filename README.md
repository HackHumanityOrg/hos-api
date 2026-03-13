# HOS API

A high-performance Rust monorepo for data indexing and API services built with modern async architecture.

## Architecture

- **API Server** - REST API with OpenAPI/Swagger documentation
- **Web App** - Next.js frontend for the veNEAR explorer
- **Indexers** - Parallel data processing services  
- **Shared Libraries** - Database, configuration, utilities, and telemetry
- **Migrations** - SeaORM-powered PostgreSQL/PlanetScale Postgres schema management
- **Observability** - Structured JSON logging and tracing

```
hos-api/
├── api/                    # REST API server with Swagger UI
├── indexers/              # Data processing services (discourse, near, telegram)
├── web/                   # Next.js frontend
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

## Deployment

This monorepo ships as independently deployable services plus a shared PostgreSQL database.

| Component | Role | Public ingress | Main requirements |
|---|---|---|---|
| `api` | Axum REST API and Swagger UI | Yes | `DATABASE_URL`, platform `PORT` or `APP_SERVER__PORT` |
| `web` | Next.js frontend | Yes | `VENEAR_API_BASE_URL` |
| `discourse-indexer` | Discourse ingestion worker | No | `DATABASE_URL` |
| `near-indexer` | NEAR ingestion worker | No | `DATABASE_URL`, optional FastNEAR credentials |
| `telegram-indexer` | Telegram ingestion worker | No | `DATABASE_URL`, Telegram credentials, durable session state |
| `migration` | One-off schema migration job | No | Run before schema-dependent rollouts |

### Recommended deployment sequence

1. Provision PostgreSQL and set `DATABASE_URL` for every Rust service.
2. Run migrations before starting the API or any indexer.
3. Deploy `api` and verify `/health` before attaching downstream services.
4. Deploy `web` with `VENEAR_API_BASE_URL` pointed at the API's reachable URL.
5. Deploy only the indexers you need for your data coverage.
6. Validate logs, health checks, and database connectivity after each rollout.

### Minimum runtime configuration

- Shared Rust services: `DATABASE_URL`, optional `RUST_LOG`, `SERVICE_VERSION`, and `DEPLOYMENT_ENVIRONMENT`
- API: optional `PORT` or `APP_SERVER__PORT`
- Web: `VENEAR_API_BASE_URL` or `NEXT_PUBLIC_VENEAR_API_BASE_URL`
- Telegram indexer: `TELEGRAM_API_ID`, `TELEGRAM_API_HASH`, `TELEGRAM_CHANNELS`, and either `TELEGRAM_SESSION_DATA` or persistent storage for `TELEGRAM_SESSION_FILE`
- NEAR indexer: optional FastNEAR credentials when you use the FastNEAR provider

### Operational notes

- The API `/health` endpoint is useful for liveness, but it still returns HTTP 200 with a `degraded` payload when the database is unavailable. Treat it as a basic health signal, not your only database readiness gate.
- The Telegram indexer defaults to a local `telegram_session.bin` file. In ephemeral container platforms, provide `TELEGRAM_SESSION_DATA` or attach durable storage so redeploys do not force re-authentication.
- The worker services are background jobs, not HTTP apps. They should rely on restart-on-failure policies, logs, and telemetry rather than path-based health checks.

### Deployment artifacts in this repo

- `api`: `/api/Dockerfile` and `/api/railway.toml`
- `web`: `/web/Dockerfile` and `/web/railway.toml`
- `discourse-indexer`: `/indexers/discourse-indexer/Dockerfile` and `/indexers/discourse-indexer/railway.toml`
- `near-indexer`: `/indexers/near-indexer/Dockerfile` and `/indexers/near-indexer/railway.toml`
- `telegram-indexer`: `/indexers/telegram-indexer/Dockerfile` and `/indexers/telegram-indexer/railway.toml`

This layout works on Railway, Fly.io, Render, Kubernetes, ECS, Nomad, or plain Docker Compose as long as the platform can run one container per service, inject environment variables, provide PostgreSQL connectivity, and expose public HTTP only for `api` and `web`.

Railway is the easiest fit today because this repo already includes one `railway.toml` per service, Dockerfile-based builds, watch patterns, and API support for Railway's injected `PORT` environment variable. If you use Railway, prefer one service per component, run migrations as a separate release step, and use private networking from `web` to `api` with a base URL like `http://${{api.RAILWAY_PRIVATE_DOMAIN}}:${{api.PORT}}/api/v1/venear`.

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
