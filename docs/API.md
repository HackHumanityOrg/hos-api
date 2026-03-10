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
RUST_LOG=debug cargo run-api
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
│   ├── state.rs             # AppState (wraps Arc<DatabaseConnection>)
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
    .with_state(app_state);
```

**Utoipa Integration**: Automatic OpenAPI documentation
```rust
#[derive(OpenApi)]
#[openapi(
    paths(handlers::health::health_check),
    components(schemas(
        types::HealthResponse,
        types::DatabaseHealth,
        errors::ApiError,
    ))
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

The `/health` endpoint always includes database connectivity information, so there is no separate database health endpoint.

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

### Identity

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/v1/identity/persons` | List persons |
| GET | `/api/v1/identity/persons/{person_id}/accounts` | List accounts for a person |

Notes:
- `GET /api/v1/identity/persons` returns a best-effort `name` for each person.
- Identity account `platform_id` values currently include `discourse`, `telegram`, `venear_governance`, and `venear_lockup`.

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
- Common shared veNEAR fields include `TokenAmountDto` payloads with `raw` and `decimal` and provenance refs with `contract_id`, `transaction_hash`, `receipt_id`, `execution_outcome_id`, `block_height`, and `block_timestamp`.
- `GET /api/v1/venear/proposals` supports `status`, `proposer_id`, `reviewer_id`, `contract_id`, `has_votes`, `q`, `created_after`, and `created_before`.
- `GET /api/v1/venear/proposals/{proposal_id}/timeline` supports `event_type` and `actor_account_id`.
- `GET /api/v1/venear/proposals/{proposal_id}/votes` supports `voter_account_id` and `vote_option`.
- `GET /api/v1/venear/proposals/{proposal_id}/vote_history` supports `voter_account_id`, `vote_option`, and `event_type`.
- `GET /api/v1/venear/accounts` supports `q`, `role`, `has_inbound_delegation`, `has_outbound_delegation`, `has_lockup`, and `min_effective_voting_power_raw`.
- `GET /api/v1/venear/accounts/{account_id}/proposals` supports `role` and `status`.
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
- `GET /api/agora/proposal/{proposal_id}/quorum` keeps the old quorum helper semantics, including `AGORA_ENV` and `QUORUM_FLOOR`, but does not apply unavailable web2 quorum overrides.
- `GET /api/agora/vote_changes/{proposal_id}` reads raw vote history from `near.near_receipt_actions` and `near.near_execution_outcomes` so repeated votes are preserved even though the materialized proposal-vote view keeps only latest rows.

## Adding New Endpoints

### 1. Create Handler Function

Create a handler in the appropriate domain module under `api/src/handlers/`. Handlers receive `State<AppState>` and call `require_db` to extract the database connection:

```rust
// api/src/handlers/discourse/instances.rs
use axum::{
    extract::{Path, Query, State},
    Json,
};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use utoipa::{IntoParams, ToSchema};
use entities::discourse_instances;
use crate::{errors::ApiErrorResponse, state::AppState};

#[derive(Serialize, ToSchema)]
pub struct InstanceResponse {
    pub instance: String,
    pub base_url: String,
}

#[derive(Deserialize, IntoParams)]
pub struct ListQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

fn require_db(state: &AppState) -> Result<Arc<DatabaseConnection>, ApiErrorResponse> {
    state.db.clone().ok_or_else(|| {
        ApiErrorResponse::service_unavailable("Database connection unavailable")
    })
}

#[utoipa::path(
    get,
    path = "/api/v1/discourse/instances",
    params(ListQuery),
    responses(
        (status = 200, description = "List instances", body = [InstanceResponse]),
        (status = 503, description = "Database unavailable"),
    )
)]
pub async fn list_instances(
    State(state): State<AppState>,
    Query(params): Query<ListQuery>,
) -> Result<Json<Vec<InstanceResponse>>, ApiErrorResponse> {
    let db = require_db(&state)?;

    let mut query = DiscourseInstances::find();
    if let Some(limit) = params.limit {
        query = query.limit(limit);
    }
    if let Some(offset) = params.offset {
        query = query.offset(offset);
    }

    let rows = query
        .all(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?;

    Ok(Json(rows.into_iter().map(|r| InstanceResponse {
        instance: r.key,
        base_url: r.base_url,
    }).collect()))
}
```

### 2. Update Router

Add the handler to the appropriate route module in `api/src/routes/`:

```rust
// api/src/routes/discourse/mod.rs
use crate::{handlers::discourse, state::AppState};
use axum::{routing::get, Router};

pub fn discourse_routes() -> Router<AppState> {
    Router::new()
        .route("/discourse/instances", get(discourse::list_instances))
        // ... existing routes ...
}
```

### 3. Update OpenAPI Documentation

Add the new path and any new schema types to `api/src/docs.rs`:

```rust
#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::health::health_check,
        handlers::discourse::list_instances,    // Add new path here
    ),
    components(
        schemas(
            types::HealthResponse,
            errors::ApiError,
            // Add new response/request types here
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
  "type": "about:blank",
  "title": "Not Found",
  "status": 404,
  "detail": "Instance 'gov.near.org' does not exist",
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

```rust
use serde::Deserialize;
use utoipa::IntoParams;

#[derive(Deserialize, IntoParams)]
pub struct PaginationQuery {
    #[param(minimum = 1, default = 1)]
    pub page: Option<u64>,
    
    #[param(minimum = 1, maximum = 100, default = 20)]
    pub limit: Option<u64>,
    
    pub sort_by: Option<String>,
}

#[utoipa::path(
    get,
    path = "/api/users",
    params(PaginationQuery),
    responses(
        (status = 200, body = [UserResponse])
    )
)]
pub async fn list_users(
    Query(params): Query<PaginationQuery>
) -> Json<Vec<UserResponse>> {
    // Implementation
}
```

### Path Parameters

```rust
#[utoipa::path(
    get,
    path = "/api/users/{id}/posts/{post_id}",
    params(
        ("id" = i32, Path, description = "User ID"),
        ("post_id" = i32, Path, description = "Post ID")
    )
)]
pub async fn get_user_post(
    Path((user_id, post_id)): Path<(i32, i32)>
) -> Json<PostResponse> {
    // Implementation
}
```

### Request Body Validation

```rust
use validator::Validate;
use serde::Deserialize;

#[derive(Deserialize, Validate, ToSchema)]
pub struct CreateUserRequest {
    #[validate(email, message = "Invalid email format")]
    pub email: String,
    
    #[validate(length(min = 2, max = 100, message = "Name must be 2-100 characters"))]
    pub name: String,
    
    #[validate(range(min = 18, max = 120, message = "Age must be 18-120"))]
    pub age: Option<u32>,
}

pub async fn create_user(
    Json(payload): Json<CreateUserRequest>
) -> Result<Json<UserResponse>, AppError> {
    // Validate request
    payload.validate()
        .map_err(|e| AppError::ValidationError(e.to_string()))?;
    
    // Process request
    // ...
}
```

## Database Integration

### Using SeaORM Entities

Handlers receive `State<AppState>` and call `require_db` (or inline the `ok_or_else` check) to obtain `Arc<DatabaseConnection>`. Pass `db.as_ref()` to SeaORM queries:

```rust
use entities::{discourse_users, prelude::*};
use sea_orm::*;
use crate::{errors::ApiErrorResponse, state::AppState};

pub async fn get_user(
    State(state): State<AppState>,
    Path(user_id): Path<i64>,
) -> Result<Json<UserResponse>, ApiErrorResponse> {
    let db = state.db.clone().ok_or_else(|| {
        ApiErrorResponse::service_unavailable("Database unavailable")
    })?;

    let user = DiscourseUsers::find_by_id(user_id)
        .one(db.as_ref())
        .await
        .map_err(ApiErrorResponse::from)?
        .ok_or_else(|| ApiErrorResponse::not_found("User not found"))?;

    Ok(Json(UserResponse { id: user.id, name: user.name }))
}
```

### Transactions

```rust
pub async fn update_with_transaction(
    State(state): State<AppState>,
    Json(payload): Json<UpdateRequest>,
) -> Result<Json<UpdateResponse>, ApiErrorResponse> {
    let db = state.db.clone().ok_or_else(|| {
        ApiErrorResponse::service_unavailable("Database unavailable")
    })?;

    let txn = db.begin().await.map_err(ApiErrorResponse::from)?;

    // Multiple database operations against &txn
    some_entity::ActiveModel { /* ... */ }
        .update(&txn)
        .await
        .map_err(ApiErrorResponse::from)?;

    txn.commit().await.map_err(ApiErrorResponse::from)?;

    Ok(Json(UpdateResponse { success: true }))
}
```

## Middleware

### Custom Middleware

```rust
use axum::{
    extract::Request,
    middleware::Next,
    response::Response,
};
use std::time::Instant;

// axum 0.8: Next is not generic — no <B> type parameter
pub async fn timing_middleware(request: Request, next: Next) -> Response {
    let start = Instant::now();
    let response = next.run(request).await;
    tracing::info!("Request took {:?}", start.elapsed());
    response
}

// Add to router
let app = Router::new()
    .route("/api/v1/discourse/instances", get(list_instances))
    .layer(axum::middleware::from_fn(timing_middleware));
```

### CORS Configuration

```rust
use tower_http::cors::{CorsLayer, Any};

let app = Router::new()
    .route("/api/users", get(list_users))
    .layer(
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    );
```

## Testing API Endpoints

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use axum_test::TestServer;
    
    #[tokio::test]
    async fn test_health_endpoint() {
        let app = create_test_app().await;
        let server = TestServer::new(app).unwrap();
        
        let response = server
            .get("/health")
            .await;
            
        response.assert_status_ok();
        response.assert_json(&json!({
            "status": "healthy"
        }));
    }
    
    #[tokio::test]
    async fn test_create_user() {
        let app = create_test_app().await;
        let server = TestServer::new(app).unwrap();
        
        let response = server
            .post("/api/users")
            .json(&json!({
                "email": "test@example.com",
                "name": "Test User"
            }))
            .await;
            
        response.assert_status(StatusCode::CREATED);
        
        let user: UserResponse = response.json();
        assert_eq!(user.email, "test@example.com");
    }
}
```

### Integration Tests

```rust
// tests/api_integration.rs
use httpc_test::{Client, Response};

#[tokio::test]
async fn test_user_crud_workflow() {
    let client = Client::new("http://localhost:3000").unwrap();
    
    // Create user
    let create_response = client
        .do_post("/api/users", json!({
            "email": "integration@test.com",
            "name": "Integration User"
        }))
        .await
        .unwrap();
    
    assert_eq!(create_response.status(), 201);
    
    let user: UserResponse = create_response.json_body().unwrap();
    
    // Get user
    let get_response = client
        .do_get(&format!("/api/users/{}", user.id))
        .await
        .unwrap();
    
    assert_eq!(get_response.status(), 200);
}
```

## Performance Considerations

### Database Connection Pooling

Configure in `config.toml`:
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
#[derive(Deserialize, IntoParams)]
pub struct PaginationQuery {
    pub page: Option<u64>,
    pub limit: Option<u64>,
}

pub async fn paginated_list(
    Query(params): Query<PaginationQuery>,
) -> Json<PaginatedResponse<UserResponse>> {
    let page = params.page.unwrap_or(1);
    let limit = params.limit.unwrap_or(20).min(100); // Cap at 100
    let offset = (page - 1) * limit;
    
    let users = Users::find()
        .limit(limit)
        .offset(offset)
        .all(&db)
        .await?;
        
    let total = Users::find().count(&db).await?;
    
    Json(PaginatedResponse {
        data: users,
        pagination: PaginationInfo {
            page,
            limit,
            total,
            has_next: (page * limit) < total,
        },
    })
}
```

---

For more information, see:
- [DEVELOPMENT.md](./DEVELOPMENT.md) - General development guide
- [MIGRATIONS.md](./MIGRATIONS.md) - Database migration guide
- Main [README.md](../README.md) - Project overview
