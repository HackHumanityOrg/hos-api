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
- **API Base URL**: http://127.0.0.1:3000
- **Swagger UI**: http://127.0.0.1:3000/swagger-ui
- **OpenAPI Spec**: http://127.0.0.1:3000/api-docs/openapi.json

### Testing Endpoints

Using curl:
```bash
# Health check
curl http://127.0.0.1:3000/health

# With pretty JSON output
curl -s http://127.0.0.1:3000/health | jq
```

Using the Swagger UI:
1. Navigate to http://127.0.0.1:3000/swagger-ui
2. Expand an endpoint
3. Click "Try it out"
4. Click "Execute"

## API Architecture

### Project Structure

```
api/
├── src/
│   ├── main.rs              # Application entry point & routing
│   ├── lib.rs               # Library exports
│   ├── handlers/            # Request handlers
│   │   └── health.rs        # Health check endpoints
│   ├── models/              # Request/Response models (future)
│   ├── middleware/          # Custom middleware (future)
│   └── error.rs             # Error handling (future)
└── Cargo.toml
```

### Core Components

**Axum Router**: Handles HTTP routing and middleware
```rust
let app = Router::new()
    .route("/health", get(health_check))
    .route("/health/db", get(database_health_check))
    .with_state(db);
```

**Utoipa Integration**: Automatic OpenAPI documentation
```rust
#[derive(OpenApi)]
#[openapi(
    paths(health_check, database_health_check),
    components(schemas(HealthResponse))
)]
struct ApiDoc;
```

**State Management**: Shared database connection
```rust
async fn handler(State(db): State<DatabaseConnection>) -> Result<Json<Response>, AppError> {
    // Use database connection
}
```

## Current Endpoints

### Health Check

**GET /health**

Returns basic health status of the API server.

```bash
curl http://127.0.0.1:3000/health
```

Response:
```json
{
  "status": "healthy",
  "timestamp": "2024-08-31T12:00:00Z",
  "version": "0.1.0"
}
```

### Database Health Check  

**GET /health/db**

Returns health status including database connectivity.

```bash
curl http://127.0.0.1:3000/health/db
```

Response (database connected):
```json
{
  "status": "healthy",
  "timestamp": "2024-08-31T12:00:00Z", 
  "version": "0.1.0",
  "database": {
    "status": "connected",
    "connection_pool": {
      "active_connections": 1,
      "idle_connections": 9
    }
  }
}
```

Response (database disconnected):
```json
{
  "status": "degraded",
  "timestamp": "2024-08-31T12:00:00Z",
  "version": "0.1.0",
  "database": {
    "status": "disconnected", 
    "error": "Connection failed: ..."
  }
}
```

## Adding New Endpoints

### 1. Create Handler Function

Create handler with Utoipa documentation:

```rust
// api/src/handlers/users.rs
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use sea_orm::*;
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use entities::{users, prelude::*};
use common::errors::DatabaseError;

#[derive(Serialize, ToSchema)]
pub struct UserResponse {
    pub id: i32,
    pub email: String,
    pub name: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Deserialize, IntoParams)]
pub struct UserQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
}

#[derive(Deserialize, ToSchema)]
pub struct CreateUserRequest {
    pub email: String,
    pub name: String,
}

#[utoipa::path(
    get,
    path = "/api/users",
    params(UserQuery),
    responses(
        (status = 200, description = "List users successfully", body = [UserResponse]),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn list_users(
    State(db): State<DatabaseConnection>,
    Query(params): Query<UserQuery>,
) -> Result<Json<Vec<UserResponse>>, StatusCode> {
    let mut query = Users::find();
    
    if let Some(limit) = params.limit {
        query = query.limit(limit);
    }
    
    if let Some(offset) = params.offset {
        query = query.offset(offset);
    }
    
    let users = query
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        
    let response: Vec<UserResponse> = users
        .into_iter()
        .map(|user| UserResponse {
            id: user.id,
            email: user.email,
            name: user.name,
            created_at: user.created_at,
        })
        .collect();
        
    Ok(Json(response))
}

#[utoipa::path(
    get,
    path = "/api/users/{id}",
    params(
        ("id" = i32, Path, description = "User ID")
    ),
    responses(
        (status = 200, description = "User found", body = UserResponse),
        (status = 404, description = "User not found"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_user(
    State(db): State<DatabaseConnection>,
    Path(id): Path<i32>,
) -> Result<Json<UserResponse>, StatusCode> {
    let user = Users::find_by_id(id)
        .one(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
        
    Ok(Json(UserResponse {
        id: user.id,
        email: user.email,
        name: user.name,
        created_at: user.created_at,
    }))
}

#[utoipa::path(
    post,
    path = "/api/users",
    request_body = CreateUserRequest,
    responses(
        (status = 201, description = "User created", body = UserResponse),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn create_user(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<CreateUserRequest>,
) -> Result<(StatusCode, Json<UserResponse>), StatusCode> {
    let new_user = users::ActiveModel {
        email: Set(payload.email),
        name: Set(payload.name),
        created_at: Set(chrono::Utc::now().naive_utc()),
        ..Default::default()
    };
    
    let user = new_user
        .insert(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        
    Ok((
        StatusCode::CREATED,
        Json(UserResponse {
            id: user.id,
            email: user.email,
            name: user.name,
            created_at: user.created_at,
        }),
    ))
}
```

### 2. Update Router

Add routes to main application:

```rust
// api/src/main.rs
mod handlers {
    pub mod health;
    pub mod users;  // Add this
}

let app = Router::new()
    .route("/health", get(handlers::health::health_check))
    .route("/health/db", get(handlers::health::database_health_check))
    .route("/api/users", get(handlers::users::list_users))
    .route("/api/users", post(handlers::users::create_user))
    .route("/api/users/:id", get(handlers::users::get_user))
    .with_state(db);
```

### 3. Update OpenAPI Documentation

```rust
#[derive(OpenApi)]
#[openapi(
    paths(
        handlers::health::health_check,
        handlers::health::database_health_check,
        handlers::users::list_users,    // Add new paths
        handlers::users::get_user,
        handlers::users::create_user,
    ),
    components(
        schemas(
            handlers::health::HealthResponse,
            handlers::users::UserResponse,     // Add new schemas
            handlers::users::CreateUserRequest,
        )
    )
)]
struct ApiDoc;
```

## Error Handling

### Standard HTTP Status Codes

The API uses standard HTTP status codes:

- **200 OK** - Successful GET requests
- **201 Created** - Successful POST requests  
- **400 Bad Request** - Invalid request data
- **404 Not Found** - Resource not found
- **500 Internal Server Error** - Server errors

### Error Response Format

```json
{
  "error": {
    "code": "VALIDATION_ERROR",
    "message": "Invalid email format",
    "details": {
      "field": "email",
      "value": "invalid-email"
    }
  }
}
```

### Creating Custom Error Types

```rust
use axum::{
    response::{IntoResponse, Response},
    http::StatusCode,
    Json,
};
use serde::Serialize;

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: ErrorDetail,
}

#[derive(Serialize)]
pub struct ErrorDetail {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

pub enum AppError {
    ValidationError(String),
    NotFound(String),
    DatabaseError(String),
    InternalError(String),
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match self {
            AppError::ValidationError(msg) => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                msg,
            ),
            AppError::NotFound(msg) => (
                StatusCode::NOT_FOUND,
                "NOT_FOUND",
                msg,
            ),
            AppError::DatabaseError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_ERROR",
                msg,
            ),
            AppError::InternalError(msg) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "INTERNAL_ERROR",
                msg,
            ),
        };

        let error_response = ErrorResponse {
            error: ErrorDetail {
                code: code.to_string(),
                message,
                details: None,
            },
        };

        (status, Json(error_response)).into_response()
    }
}
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

```rust
use entities::{users, posts, prelude::*};
use sea_orm::*;

pub async fn get_user_with_posts(
    State(db): State<DatabaseConnection>,
    Path(user_id): Path<i32>,
) -> Result<Json<UserWithPostsResponse>, AppError> {
    // Find user with related posts
    let user = Users::find_by_id(user_id)
        .find_with_related(Posts)
        .all(&db)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?
        .into_iter()
        .next()
        .ok_or(AppError::NotFound("User not found".to_string()))?;

    // Transform to response format
    let (user, posts) = user;
    let response = UserWithPostsResponse {
        id: user.id,
        email: user.email,
        name: user.name,
        posts: posts.into_iter().map(|p| PostResponse {
            id: p.id,
            title: p.title,
            content: p.content,
            created_at: p.created_at,
        }).collect(),
    };

    Ok(Json(response))
}
```

### Transactions

```rust
pub async fn transfer_user_data(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<TransferRequest>,
) -> Result<Json<TransferResponse>, AppError> {
    let txn = db.begin().await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    // Multiple database operations
    let user = users::ActiveModel {
        id: Set(payload.user_id),
        status: Set("transferred".to_string()),
        ..Default::default()
    };
    
    user.update(&txn).await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    // More operations...

    txn.commit().await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(Json(TransferResponse { success: true }))
}
```

## Middleware

### Custom Middleware

```rust
use axum::{
    middleware::Next,
    http::{Request, Response},
    response::IntoResponse,
};
use std::time::Instant;

pub async fn timing_middleware<B>(
    request: Request<B>,
    next: Next<B>,
) -> impl IntoResponse {
    let start = Instant::now();
    
    let response = next.run(request).await;
    
    let elapsed = start.elapsed();
    tracing::info!("Request took {:?}", elapsed);
    
    response
}

// Add to router
let app = Router::new()
    .route("/api/users", get(list_users))
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
    let client = Client::new("http://127.0.0.1:3000").unwrap();
    
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

```rust
use axum::response::Response;
use tower_http::cache::{CacheLayer, CacheLayerBuilder};

let app = Router::new()
    .route("/api/users", get(list_users))
    .layer(
        CacheLayerBuilder::new()
            .max_age(Duration::from_secs(300)) // 5 minutes
            .build()
    );
```

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