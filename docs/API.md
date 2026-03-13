# API Documentation

This guide covers the REST API server built with Axum and automatic OpenAPI documentation generation.

## Overview

The API server provides:
- **REST endpoints** with JSON responses
- **Automatic OpenAPI** documentation via Utoipa
- **Interactive Swagger UI** for testing
- **Type-safe** request/response handling
- **Database integration** via SeaORM
- **Structured error handling**

## Quick Start

### Running the API Server

```bash
# Start the API server
cargo run-api

# Or with specific configuration
APP_LOGGING__LEVEL=debug cargo run-api
```

The server will be available at:
- **API Base URL**: http://localhost:3000
- **Swagger UI**: http://localhost:3000/swagger-ui
- **OpenAPI Spec**: http://localhost:3000/api-docs/openapi.json

### Testing Endpoints

Using curl:
```bash
# Health check
curl http://localhost:3000/health

# With pretty JSON output
curl -s http://localhost:3000/health | jq
```

Using the Swagger UI:
1. Navigate to http://localhost:3000/swagger-ui
2. Expand an endpoint
3. Click "Try it out"
4. Click "Execute"

## API Architecture

### Project Structure

```
api/
├── src/
│   ├── main.rs              # Application entry point
│   ├── lib.rs               # Library exports
│   ├── state.rs             # AppState (holds an optional Arc<DatabaseConnection>)
│   ├── errors.rs            # ApiError / ApiErrorResponse (RFC 9457)
│   ├── types.rs             # Shared request/response models
│   ├── docs.rs              # OpenAPI struct (ApiDoc via Utoipa)
│   ├── routes.rs            # Top-level router wiring
│   ├── handlers/            # Request handlers
│   │   ├── discourse/       # Discourse forum handlers
│   │   ├── telegram/        # Telegram channel handlers
│   │   ├── identity/        # Identity / person handlers
│   │   ├── venear/          # veNEAR governance handlers
│   │   ├── agora/           # Agora compatibility handlers
│   │   └── health/          # Health check handler
│   └── routes/              # Route definitions
│       ├── discourse/       # Discourse route module
│       ├── telegram/        # Telegram route module
│       ├── identity/        # Identity route module
│       ├── venear/          # veNEAR route module
│       ├── agora/           # Agora compatibility route module
│       └── health/          # Health route module
└── Cargo.toml
```

### Core Components

**Axum Router**: Handles HTTP routing and middleware
```rust
let app = Router::new()
    .nest("/api/v1", routes::create_routes())
    .nest("/api/agora", routes::agora::agora_routes())
    .merge(routes::health::health_routes())
    .layer(TraceLayer::new_for_http())
    .with_state(app_state)
    .merge(
        SwaggerUi::new("/swagger-ui")
            .url("/api-docs/openapi.json", docs::ApiDoc::openapi())
    );
```

**Utoipa Integration**: Automatic OpenAPI documentation
```rust
#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::health::health_check,
        handlers::identity::list_persons,
        handlers::discourse::list_instances,
    ),
    components(schemas(
        types::HealthResponse,
        types::DatabaseHealth,
        errors::ApiError,
    )),
    tags(
        (name = "health", description = "Service and dependency health diagnostics"),
        (name = "identity", description = "Identity read endpoints"),
        (name = "discourse", description = "Discourse read endpoints"),
    )
)]
struct ApiDoc;
```

**State Management**: Shared application state wrapping an optional `Arc<DatabaseConnection>`
```rust
// api/src/state.rs
pub struct AppState {
    pub db: Option<Arc<DatabaseConnection>>,
}

// In a handler — use require_db to extract db or return 503
async fn handler(State(state): State<AppState>) -> Result<Json<Response>, ApiErrorResponse> {
    let db = state.db.clone().ok_or_else(|| {
        ApiErrorResponse::service_unavailable("Database connection unavailable")
    })?;
    // db: Arc<DatabaseConnection>
    let rows = SomeEntity::find().all(db.as_ref()).await.map_err(ApiErrorResponse::from)?;
    Ok(Json(rows))
}
```

## Current Endpoints

Primary read endpoints are nested under `/api/v1`. The legacy Agora compatibility surface is mounted separately under `/api/agora`. The health endpoint lives at the root.

### Health

| Method | Path | Description |
|--------|------|-------------|
| GET | `/health` | Server and database health status |

The `/health` endpoint always includes database connectivity information, so
there is no separate database health endpoint. It still returns HTTP `200` when
the payload status is `degraded`, which makes it useful for liveness checks but
not a complete readiness gate.

```bash
curl http://localhost:3000/health
```

Response:
```json
{
  "status": "healthy",
  "timestamp": "2024-08-31T12:00:00Z",
  "version": "0.1.0",
  "database": {
    "connected": true,
    "response_time_ms": 15
  }
}
```

### Discourse

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/discourse/instances` | List all indexed Discourse instances |
| GET | `/api/v1/discourse/instances/{instance}` | Get a specific instance |
| GET | `/api/v1/discourse/instances/{instance}/categories` | List categories |
| GET | `/api/v1/discourse/instances/{instance}/categories/{category_id}` | Get a category |
| GET | `/api/v1/discourse/instances/{instance}/topics` | List topics |
| GET | `/api/v1/discourse/instances/{instance}/topics/{topic_id}` | Get a topic |
| GET | `/api/v1/discourse/instances/{instance}/topics/{topic_id}/posts` | List posts in a topic |
| GET | `/api/v1/discourse/instances/{instance}/posts/{post_id}` | Get a post |
| GET | `/api/v1/discourse/instances/{instance}/posts/{post_id}/likes` | List post likes |
| GET | `/api/v1/discourse/instances/{instance}/posts/{post_id}/revisions` | List post revisions |
| GET | `/api/v1/discourse/instances/{instance}/users` | List users |
| GET | `/api/v1/discourse/instances/{instance}/users/{user_id}` | Get a user |
| GET | `/api/v1/discourse/instances/{instance}/users/{user_id}/posts` | List posts by a user |
| GET | `/api/v1/discourse/instances/{instance}/sync-lag` | Get indexer sync lag for instance |

Notes:
- All Discourse list endpoints use cursor pagination via `limit` and `cursor`.
- `GET /api/v1/discourse/instances/{instance}/categories` supports `parent_category_id` and `q`.
- `GET /api/v1/discourse/instances/{instance}/topics` supports `q`, `category_id`, `pinned`, `closed`, `archived`, `visible`, `bumped_after`, and `bumped_before`.
- `GET /api/v1/discourse/instances/{instance}/topics/{topic_id}/posts` supports `username` and `q`.
- `GET /api/v1/discourse/instances/{instance}/users` supports `q` and `trust_level`.

### Telegram

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/telegram/channels` | List all indexed channels |
| GET | `/api/v1/telegram/channels/{channel_telegram_id}` | Get a channel |
| GET | `/api/v1/telegram/channels/{channel_telegram_id}/messages` | List messages |
| GET | `/api/v1/telegram/channels/{channel_telegram_id}/messages/{telegram_message_id}` | Get a message |
| GET | `/api/v1/telegram/channels/{channel_telegram_id}/messages/{telegram_message_id}/reactions` | List message reactions |
| GET | `/api/v1/telegram/channels/{channel_telegram_id}/messages/{telegram_message_id}/service-actions` | List service actions |
| GET | `/api/v1/telegram/users` | List users |
| GET | `/api/v1/telegram/users/{user_telegram_id}` | Get a user |
| GET | `/api/v1/telegram/users/{user_telegram_id}/messages` | List messages by user |

Notes:
- All Telegram list endpoints use cursor pagination via `limit` and `cursor`.
- `GET /api/v1/telegram/channels` supports `q`, `channel_type`, and `username`.
- `GET /api/v1/telegram/users` supports `q`.
- `GET /api/v1/telegram/channels/{channel_telegram_id}/messages` supports `q`, `user_telegram_id`, `message_type`, `has_media`, `date_after`, and `date_before`.
- `GET /api/v1/telegram/channels/{channel_telegram_id}/messages/{telegram_message_id}/reactions` supports `emoji` and `user_telegram_id`.
- `GET /api/v1/telegram/channels/{channel_telegram_id}/messages/{telegram_message_id}/service-actions` supports `action` and `actor_telegram_id`.
- `GET /api/v1/telegram/users/{user_telegram_id}/messages` supports `channel_telegram_id`, `q`, `message_type`, `has_media`, `date_after`, and `date_before`.

### Identity

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/identity/persons` | List persons |
| GET | `/api/v1/identity/persons/{person_id}/accounts` | List accounts for a person |

Notes:
- `GET /api/v1/identity/persons` uses cursor pagination via `limit` and `cursor`.
- `GET /api/v1/identity/persons` returns a best-effort `name` for each person.
- Identity account `platform_id` values are not fixed to a short closed list. Current examples in this repo include `near`, `discourse`, and `telegram`.

### veNEAR Governance

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/venear/proposals` | List canonical proposal summaries |
| GET | `/api/v1/venear/proposals/{proposal_id}` | Get full canonical proposal detail |
| GET | `/api/v1/venear/proposals/{proposal_id}/timeline` | List canonical proposal lifecycle events |
| GET | `/api/v1/venear/proposals/{proposal_id}/votes` | List current effective votes for a proposal |
| GET | `/api/v1/venear/proposals/{proposal_id}/vote_history` | List full successful vote history for a proposal |
| GET | `/api/v1/venear/accounts` | List account-centric veNEAR summaries |
| GET | `/api/v1/venear/accounts/{account_id}` | Get current account governance state |
| GET | `/api/v1/venear/accounts/{account_id}/proposals` | List proposal summaries related to one account |
| GET | `/api/v1/venear/accounts/{account_id}/votes` | List current effective votes cast by one account |
| GET | `/api/v1/venear/accounts/{account_id}/vote_history` | List full successful vote history for one account |
| GET | `/api/v1/venear/accounts/{account_id}/voting_power_history` | List derived voting-power mutations for one account |
| GET | `/api/v1/venear/accounts/{account_id}/delegation_history` | List delegation events involving one account |
| GET | `/api/v1/venear/accounts/{account_id}/lockup_events` | List lockup lifecycle events for one account |
| GET | `/api/v1/venear/accounts/{account_id}/rewards_events` | List reward and claim events for one account |
| GET | `/api/v1/venear/delegations` | List current active delegation edges |
| GET | `/api/v1/venear/lockups` | List current lockup summaries |
| GET | `/api/v1/venear/lockups/{lockup_account_id}` | Get current lockup detail |
| GET | `/api/v1/venear/lockups/{lockup_account_id}/events` | List lockup lifecycle history |
| GET | `/api/v1/venear/config` | Get current config snapshots by contract |
| GET | `/api/v1/venear/config/history` | List config change history from governance method calls |
| GET | `/api/v1/venear/admin/actions` | List admin and operator actions, including failures |
| GET | `/api/v1/venear/activity` | List the normalized cross-domain activity feed |

Notes:
- All veNEAR list endpoints support cursor pagination via `limit` and `cursor`.
- The `/api/v1/venear` surface now reads directly from canonical NEAR tables and does not depend on `near.hos_*` materialized views.
- Common shared veNEAR fields include `VenearTokenAmountDto` payloads with `raw`, `decimal`, `symbol`, and `decimals`, plus provenance refs with `contract_id`, `transaction_hash`, `receipt_id`, `execution_outcome_id`, `block_height`, and `block_timestamp`.
- `GET /api/v1/venear/proposals` supports `status`, `proposer_id`, `reviewer_id`, `contract_id`, `has_votes`, `q`, `created_after`, and `created_before`.
- `GET /api/v1/venear/proposals/{proposal_id}/timeline` supports `event_type` and `actor_account_id`.
- `GET /api/v1/venear/proposals/{proposal_id}/votes` supports `voter_account_id` and `vote_option`.
- `GET /api/v1/venear/proposals/{proposal_id}/vote_history` supports `voter_account_id`, `vote_option`, and `event_type`.
- `GET /api/v1/venear/accounts` supports `q`, `role`, `has_inbound_delegation`, `has_outbound_delegation`, `has_lockup`, and `min_effective_voting_power_raw`.
- `GET /api/v1/venear/accounts/{account_id}/proposals` supports `role` and `status`.
- `GET /api/v1/venear/accounts/{account_id}/votes` supports `status`.
- `GET /api/v1/venear/accounts/{account_id}/vote_history` supports `proposal_id` and `event_type`.
- `GET /api/v1/venear/accounts/{account_id}/voting_power_history` supports `reason` and `proposal_id`.
- `GET /api/v1/venear/accounts/{account_id}/delegation_history` supports `direction`, `event_type`, and `counterparty_account_id`.
- `GET /api/v1/venear/accounts/{account_id}/lockup_events` and `/rewards_events` support `event_type`.
- `GET /api/v1/venear/delegations` supports `delegator_account_id` and `delegatee_account_id`.
- `GET /api/v1/venear/lockups` supports `owner_account_id`, `status`, and `has_delegatee`.
- `GET /api/v1/venear/config/history` supports `contract_id`, `field`, and `method_name`.
- `GET /api/v1/venear/admin/actions` supports `contract_id`, `event_type`, `actor_account_id`, and `success`.
- `GET /api/v1/venear/activity` supports `kind`, `account_id`, `proposal_id`, `contract_id`, `event_type`, and `success`.

### Agora Compatibility

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/agora/proposal/approved` | List approved proposals in the legacy Agora payload shape |
| GET | `/api/agora/proposal/pending` | List pending proposals in the legacy Agora payload shape |
| GET | `/api/agora/proposal/{proposal_id}/votes` | List proposal votes |
| GET | `/api/agora/proposal/{proposal_id}/non-voters` | List proposal non-voters |
| GET | `/api/agora/proposal/{proposal_id}/charts` | Get proposal chart data |
| GET | `/api/agora/proposal/{proposal_id}/quorum` | Get proposal quorum |
| GET | `/api/agora/delegates` | List delegates |
| GET | `/api/agora/delegates/{address}` | Get one delegate |
| GET | `/api/agora/delegates/{address}/voting-history` | List delegate voting history |
| GET | `/api/agora/delegates/{address}/delegated-from` | List inbound delegation rows for a delegate |
| GET | `/api/agora/delegates/{address}/delegated-to` | List outbound delegation rows for a delegate |
| GET | `/api/agora/delegates/{address}/hos-activity` | List delegate HOS activity |
| GET | `/api/agora/delegate_statement_changes` | Return the legacy delegate-statement change shape with empty on-chain-only data |
| GET | `/api/agora/get_voting_power_chart/{account_id}` | Get legacy voting-power chart points |
| GET | `/api/agora/vote_changes/{proposal_id}` | Get grouped multi-vote history for voters who changed votes |

Notes:
- The Agora namespace preserves the old query parameter contract: `page` and `page_size` pagination, legacy camelCase payloads, and simple error envelopes such as `{ "error": ... }` and `{ "message": ... }`.
- This compatibility layer is backed by local NEAR/HOS indexed data only. Fields that used to depend on `web2` tables are returned as `null`, and `delegate_statement_changes` returns an empty result set in the original shape.
- `GET /api/agora/proposal/pending` also supports `created_by`.
- `GET /api/agora/delegates` also supports `order_by`, `filter_by`, `issue_type`, and `sorting_seed`.
- `GET /api/agora/delegate_statement_changes` uses `page`, `offset`, and `lookback_days`, and keeps its original response envelope instead of the `/api/v1` cursor shape.
- `GET /api/agora/delegates/{address}/hos-activity` also accepts legacy `network_id` and `contract_id` query params.
- `GET /api/agora/proposal/{proposal_id}/quorum` keeps the old quorum helper semantics, including `AGORA_ENV` and `QUORUM_FLOOR`, but does not apply unavailable web2 quorum overrides.
- `GET /api/agora/vote_changes/{proposal_id}` reads raw vote history from `near.near_receipt_actions` and `near.near_execution_outcomes` so repeated votes are preserved even though the materialized proposal-vote view keeps only latest rows.

## Adding New Endpoints

The current API surface is read-only. New routes usually follow this sequence:

1. Add a handler in the relevant domain module under `api/src/handlers/`.
2. Wire the path in the matching module under `api/src/routes/`.
3. Register the path and any new DTOs in `api/src/docs.rs`.

Example handler pattern:

```rust
use crate::{errors::ApiErrorResponse, state::AppState, types::*};
use axum::{
    extract::{Query, State},
    Json,
};
use entities::{identity::persons, prelude::*};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder};
use std::sync::Arc;

fn require_db(app_state: &AppState) -> Result<Arc<DatabaseConnection>, ApiErrorResponse> {
    app_state
        .db
        .clone()
        .ok_or_else(|| ApiErrorResponse::service_unavailable("Database connection unavailable"))
}

#[utoipa::path(
    get,
    path = "/api/v1/identity/persons",
    params(CursorPaginationQuery),
    responses(
        (status = 200, description = "Identity persons", body = IdentityPersonsListResponse),
        (status = 400, description = "Invalid cursor or query value", body = crate::errors::ApiError),
        (status = 503, description = "Database unavailable", body = crate::errors::ApiError)
    )
)]
pub async fn list_persons(
    State(app_state): State<AppState>,
    Query(query): Query<CursorPaginationQuery>,
) -> Result<Json<IdentityPersonsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = query.limit.unwrap_or(20).clamp(1, 100);

    let builder = Persons::find().order_by_asc(persons::Column::Id);

    let rows = builder
        .limit(limit + 1)
        .all(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?;

    // Decode and apply the cursor filter before the fetch when the endpoint
    // needs stable pagination across ordered rows, then encode `next_cursor`
    // from the last retained row.
    let _ = rows;
    Ok(Json(IdentityPersonsListResponse {
        data: vec![],
        next_cursor: None,
        has_more: false,
    }))
}
```

Route wiring happens in the domain route module, not in `main.rs`:

```rust
use crate::{handlers::identity, state::AppState};
use axum::{routing::get, Router};

pub fn identity_routes() -> Router<AppState> {
    Router::new()
        .route("/identity/persons", get(identity::list_persons))
        .route(
            "/identity/persons/{person_id}/accounts",
            get(identity::list_person_accounts),
        )
}
```

OpenAPI registration stays centralized in `api/src/docs.rs`:

```rust
#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::health::health_check,
        handlers::identity::list_persons,
        handlers::identity::list_person_accounts,
    ),
    components(
        schemas(
            types::HealthResponse,
            types::IdentityPersonsListResponse,
            types::IdentityPersonAccountsResponse,
            errors::ApiError,
        )
    )
)]
pub struct ApiDoc;
```

## Error Handling

### Standard HTTP Status Codes

The API uses standard HTTP status codes:

- **200 OK** - Successful GET requests
- **400 Bad Request** - Invalid request data
- **401 Unauthorized** - Authentication required
- **403 Forbidden** - Insufficient permissions
- **404 Not Found** - Resource not found
- **422 Unprocessable Entity** - Validation error
- **500 Internal Server Error** - Server errors
- **503 Service Unavailable** - Database unavailable

### Error Response Format

Errors follow [RFC 9457 Problem Details](https://www.rfc-editor.org/rfc/rfc9457) and are returned with `Content-Type: application/problem+json`:

```json
{
  "type": "https://hos-api.dev/problems/not-found",
  "title": "Identity person not found",
  "status": 404,
  "code": "NOT_FOUND"
}
```

Fields `detail`, `instance`, and `code` are omitted when absent.

### Using `ApiErrorResponse` in Handlers

All handlers return `Result<Json<T>, ApiErrorResponse>`. The `ApiErrorResponse` type provides convenience constructors:

```rust
use crate::errors::ApiErrorResponse;

// Common constructors (status code + machine-readable code set automatically)
ApiErrorResponse::not_found("Instance not found")           // 404 NOT_FOUND
ApiErrorResponse::bad_request("Missing required field")     // 400 BAD_REQUEST
ApiErrorResponse::service_unavailable("DB unavailable")     // 503 SERVICE_UNAVAILABLE
ApiErrorResponse::internal_server_error("Unexpected error") // 500 INTERNAL_ERROR
ApiErrorResponse::unauthorized("Token required")            // 401 UNAUTHORIZED
ApiErrorResponse::forbidden("Insufficient permissions")     // 403 FORBIDDEN
ApiErrorResponse::unprocessable_entity("Validation failed") // 422 VALIDATION_ERROR

// Add human-readable detail with builder chaining
ApiErrorResponse::not_found("Instance not found")
    .with_detail(format!("No instance with id '{instance}'"))

// Convert from sea_orm::DbErr directly
.map_err(ApiErrorResponse::from)?
```

## Request/Response Patterns

### Query Parameters

The primary `/api/v1` list endpoints use cursor pagination rather than page/offset pagination.

```rust
#[derive(Debug, Deserialize, IntoParams)]
pub struct CursorPaginationQuery {
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/v1/identity/persons",
    params(CursorPaginationQuery),
    responses(
        (status = 200, body = IdentityPersonsListResponse),
        (status = 400, body = crate::errors::ApiError),
        (status = 503, body = crate::errors::ApiError),
    )
)]
pub async fn list_persons(
    State(app_state): State<AppState>,
    Query(query): Query<CursorPaginationQuery>,
) -> Result<Json<IdentityPersonsListResponse>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let _ = (db, limit);
    Ok(Json(IdentityPersonsListResponse {
        data: vec![],
        next_cursor: None,
        has_more: false,
    }))
}
```

For these endpoints, clients should:

- Omit `cursor` on the first request.
- Reuse `next_cursor` exactly as returned by the previous page.
- Continue until `next_cursor` is `null` or `has_more` is `false`.

### Path Parameters

```rust
#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances/{instance}/topics/{topic_id}",
    params(
        ("instance" = String, Path, description = "Discourse instance key"),
        ("topic_id" = i32, Path, description = "Discourse topic id")
    )
)]
pub async fn get_topic(
    State(app_state): State<AppState>,
    Path((instance, topic_id)): Path<(String, i32)>,
) -> Result<Json<DiscourseTopicDto>, ApiErrorResponse> {
    let db = require_db(&app_state)?;
    let _ = (db, instance, topic_id);
    unimplemented!("example only")
}
```

### Read-only Contract

The current API surface is read-only. New endpoints should generally expose indexed data through:

- path parameters for stable resource identifiers
- query parameters for filtering and pagination
- typed DTO responses documented through `utoipa`

Write flows, mutations, and bulk backfills belong in indexers or migration/backfill binaries rather than in the API crate.

## Database Integration

### Using SeaORM Entities

Handlers receive `State<AppState>` and call `require_db` (or inline the `ok_or_else` check) to obtain `Arc<DatabaseConnection>`. Pass `db.as_ref()` to SeaORM queries:

```rust
use entities::{identity::persons, prelude::*};
use sea_orm::*;
use crate::{errors::ApiErrorResponse, state::AppState};

pub async fn get_person(
    State(state): State<AppState>,
    Path(person_id): Path<i64>,
) -> Result<Json<IdentityPersonDto>, ApiErrorResponse> {
    let db = state.db.clone().ok_or_else(|| {
        ApiErrorResponse::service_unavailable("Database unavailable")
    })?;

    let person = Persons::find_by_id(person_id)
        .one(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::not_found("Identity person not found"))?;

    Ok(Json(IdentityPersonDto {
        id: person.id,
        name: person.name,
        created_at: person.created_at.and_utc().to_rfc3339(),
        updated_at: person.updated_at.and_utc().to_rfc3339(),
    }))
}
```

### Read-only Handlers

The API handlers query indexed tables and map failures through `ApiErrorResponse`. They do not currently open write transactions; all domain writes happen in indexers and migration/backfill binaries.

## Middleware

### Current Middleware

```rust
let app = Router::new()
    .nest("/api/v1", routes::create_routes())
    .nest("/api/agora", routes::agora::agora_routes())
    .merge(routes::health::health_routes())
    .layer(TraceLayer::new_for_http());
```

There is no committed CORS layer in `api/src/main.rs`. If you need browser-facing cross-origin access, add `CorsLayer` there or terminate it at the reverse proxy.

## Testing API Endpoints

### Route Wiring Tests

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
    let response = support::build_router_with_state(None)
        .into_service()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}
```

### PostgreSQL-backed E2E Tests

```rust
use api::types::HealthResponse;
use axum::{body::Body, http::Request};
use tower::ServiceExt;

#[path = "../support/mod.rs"]
mod support;

#[tokio::test]
async fn health_endpoint_reports_healthy_with_live_database() {
    let (router, _database) = support::router_with_database().await;
    let response = router
        .into_service()
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

## Performance Considerations

### Database Connection Pooling

Configure in `api/config.toml`:
```toml
[database]
max_connections = 20
min_connections = 5
connection_timeout = 30
idle_timeout = 600
```

### Response Caching

The project does not implement in-process response caching. HTTP-level caching (e.g., `Cache-Control` headers for Railway's edge or a Cloudflare proxy) should be handled at the reverse-proxy layer rather than inside the Axum application.

### Pagination

```rust
#[derive(Debug, Deserialize, IntoParams)]
pub struct CursorPaginationQuery {
    pub limit: Option<u64>,
    pub cursor: Option<String>,
}

pub async fn paginated_list(
    Query(params): Query<CursorPaginationQuery>,
) -> Result<Json<InstancesListResponse>, ApiErrorResponse> {
    let limit = params.limit.unwrap_or(20).clamp(1, 100);
    let builder = DiscourseInstances::find().order_by_asc(discourse_instances::Column::Key);
    let mut rows = builder.limit(limit + 1).all(db.as_ref()).await?;

    let has_more = rows.len() > limit as usize;
    if has_more {
        rows.pop();
    }

    let _ = (rows, has_more);
    Ok(Json(InstancesListResponse {
        data: vec![],
        next_cursor: None,
        has_more: false,
    }))
}
```

---

For more information, see:
- [DEVELOPMENT.md](./DEVELOPMENT.md) - General development guide
- [MIGRATIONS.md](./MIGRATIONS.md) - Database migration guide
- Main [README.md](../README.md) - Project overview
