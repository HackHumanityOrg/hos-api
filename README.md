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

### Docker

Use plain Docker when you want to run each container explicitly instead of relying on [`compose.yml`](compose.yml).
This workflow publishes the same host ports as Docker Compose, so stop one stack before starting the other.

Build the base images:

```bash
docker build -f migration/Dockerfile -t hos-api/migration .
docker build -f api/Dockerfile -t hos-api/api .
docker build -f web/Dockerfile -t hos-api/web .
```

Create a network and start PostgreSQL:

```bash
docker network create hos-api-local
docker volume create hos-api-postgres

docker run -d \
  --name hos-api-db \
  --network hos-api-local \
  -p 55432:5432 \
  -e POSTGRES_DB=hos_api \
  -e POSTGRES_USER=hos \
  -e POSTGRES_PASSWORD=hos \
  -v hos-api-postgres:/var/lib/postgresql/data \
  postgres:16-alpine
```

Wait for PostgreSQL and run migrations:

```bash
until docker exec hos-api-db pg_isready -U hos -d hos_api; do sleep 1; done

docker run --rm \
  --name hos-api-migrate \
  --network hos-api-local \
  -e DATABASE_URL=postgresql://hos:hos@hos-api-db:5432/hos_api \
  hos-api/migration up
```

Start the API and web containers:

```bash
docker run -d \
  --name hos-api-api \
  --network hos-api-local \
  -p 3000:3000 \
  -e DATABASE_URL=postgresql://hos:hos@hos-api-db:5432/hos_api \
  -e APP_LOGGING__LEVEL=info \
  -e PORT=3000 \
  hos-api/api

docker run -d \
  --name hos-api-web \
  --network hos-api-local \
  -p 3001:3000 \
  -e VENEAR_API_BASE_URL=http://hos-api-api:3000/api/v1/venear \
  -e NEXT_PUBLIC_VENEAR_API_BASE_URL=http://hos-api-api:3000/api/v1/venear \
  hos-api/web
```

Optional indexers:

```bash
docker build -f indexers/discourse-indexer/Dockerfile -t hos-api/discourse-indexer .
docker run -d \
  --name hos-api-discourse-indexer \
  --network hos-api-local \
  -e DATABASE_URL=postgresql://hos:hos@hos-api-db:5432/hos_api \
  -e APP_LOGGING__LEVEL=info \
  hos-api/discourse-indexer

docker build -f indexers/near-indexer/Dockerfile -t hos-api/near-indexer .
docker run -d \
  --name hos-api-near-indexer \
  --network hos-api-local \
  -e DATABASE_URL=postgresql://hos:hos@hos-api-db:5432/hos_api \
  -e APP_LOGGING__LEVEL=info \
  -e APP_NEAR__FASTNEAR_NUM_THREADS=2 \
  -e APP_NEAR__PROVIDER=fastnear \
  -e APP_NEAR__FASTNEAR_API_KEY="${FASTNEAR_API_KEY:-}" \
  hos-api/near-indexer
```

Telegram requires credentials and persistent session storage:

```bash
docker build -f indexers/telegram-indexer/Dockerfile -t hos-api/telegram-indexer .
docker volume create hos-api-telegram-session

docker run -d \
  --name hos-api-telegram-indexer \
  --network hos-api-local \
  -e DATABASE_URL=postgresql://hos:hos@hos-api-db:5432/hos_api \
  -e TELEGRAM_API_ID=... \
  -e TELEGRAM_API_HASH=... \
  -e TELEGRAM_CHANNELS=... \
  -e TELEGRAM_SESSION_FILE=/var/lib/telegram/telegram_session.bin \
  -v hos-api-telegram-session:/var/lib/telegram \
  hos-api/telegram-indexer
```

Useful checks:

```bash
docker ps --filter name=hos-api
docker logs -f hos-api-api
curl http://127.0.0.1:3000/health
curl http://127.0.0.1:3001/health
```

Stop and clean up:

```bash
docker rm -f \
  hos-api-web \
  hos-api-api \
  hos-api-discourse-indexer \
  hos-api-near-indexer \
  hos-api-telegram-indexer \
  hos-api-db \
  2>/dev/null || true

docker network rm hos-api-local 2>/dev/null || true

# Optional: also remove local persistent state.
docker volume rm hos-api-postgres hos-api-telegram-session 2>/dev/null || true
```

### Docker Compose

The repo now includes [`compose.yml`](compose.yml) for a full local container stack:

- `db` starts PostgreSQL 16 with a named data volume
- `migrate` runs `migration up` before any schema-dependent service starts
- `api` and `web` are enabled by default
- `discourse-indexer`, `near-indexer`, and `telegram-indexer` are opt-in profiles

This workflow publishes the same host ports as the plain Docker commands above, so stop one stack before starting the other.

Build the images:

```bash
docker compose build
```

Start the default stack:

```bash
docker compose up -d
```

Access the services:

- API: `http://127.0.0.1:3000`
- Swagger UI: `http://127.0.0.1:3000/swagger-ui/`
- Web: `http://127.0.0.1:3001`
- PostgreSQL from the host: `postgresql://hos:hos@127.0.0.1:55432/hos_api`

Enable the indexers when you want them:

```bash
docker compose --profile discourse up -d discourse-indexer
docker compose --profile near up -d near-indexer
TELEGRAM_API_ID=... TELEGRAM_API_HASH=... TELEGRAM_CHANNELS=... \
  docker compose --profile telegram up -d telegram-indexer
```

Useful checks:

```bash
docker compose ps
docker compose logs -f migrate api web
curl http://127.0.0.1:3000/health
curl http://127.0.0.1:3001/health
```

Stop the stack:

```bash
# Include the optional profiles so any profile containers you started are removed too.
docker compose --profile discourse --profile near --profile telegram down
docker compose --profile discourse --profile near --profile telegram down -v
```

Notes for this compose stack:

- The compose file intentionally uses `COMPOSE_DATABASE_URL` instead of inheriting the root `.env` `DATABASE_URL`. That avoids accidentally pointing containers at a host-local database.
- `web/.env.local` is excluded from Docker build context, so local frontend env files no longer leak into image builds.
- The `near-indexer` profile lowers FastNEAR fetch threads to `2` by default. It still benefits from `FASTNEAR_API_KEY`, or you can switch to `NEAR_PROVIDER=lake`.
- The `telegram-indexer` profile stores session state in the `telegram-session` named volume via `TELEGRAM_SESSION_FILE=/var/lib/telegram/telegram_session.bin`.

### Recommended deployment sequence

1. Provision PostgreSQL and set `DATABASE_URL` for every Rust service.
2. Run the `migration` container before starting the API or any indexer.
3. Deploy `api` and verify `/health` before attaching downstream services.
4. Deploy `web` with `VENEAR_API_BASE_URL` pointed at the API's reachable URL.
5. Deploy only the indexers you need for your data coverage.
6. Validate logs, health checks, and database connectivity after each rollout.

### Minimum runtime configuration

- Shared Rust services: `DATABASE_URL`
- Common optional overrides: `APP_LOGGING__LEVEL`, `APP_LOGGING__FORMAT`, `APP_TELEMETRY__ENVIRONMENT`, and `APP_TELEMETRY__SERVICE_VERSION`
- API: optional `PORT` or `APP_SERVER__PORT`
- Web: `VENEAR_API_BASE_URL` or `NEXT_PUBLIC_VENEAR_API_BASE_URL`
- Telegram indexer: `TELEGRAM_API_ID`, `TELEGRAM_API_HASH`, `TELEGRAM_CHANNELS`, and either `TELEGRAM_SESSION_DATA` or persistent storage for `TELEGRAM_SESSION_FILE`
- NEAR indexer: optional `FASTNEAR_API_KEY`, or set `APP_NEAR__PROVIDER=lake` if you do not want FastNEAR

### Operational notes

- The API `/health` endpoint is useful for liveness, but it still returns HTTP 200 with a `degraded` payload when the database is unavailable. Treat it as a basic health signal, not your only database readiness gate.
- The Telegram indexer defaults to a local `telegram_session.bin` file. In ephemeral container platforms, provide `TELEGRAM_SESSION_DATA` or attach durable storage so redeploys do not force re-authentication.
- The worker services are background jobs, not HTTP apps. They should rely on restart-on-failure policies, logs, and telemetry rather than path-based health checks.
- The NEAR and Discourse workers can hit upstream rate limits during initial backfills. For production rollouts, expect to tune concurrency and provider credentials rather than treating the defaults as infinite-throughput settings.

### Deployment artifacts in this repo

- `compose`: `/compose.yml`
- `api`: `/api/Dockerfile` and `/api/railway.toml`
- `web`: `/web/Dockerfile` and `/web/railway.toml`
- `migration`: `/migration/Dockerfile`
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
