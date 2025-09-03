// Swagger UI and OpenAPI routes smoke tests
use axum::Router;
use axum_test::TestServer;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

// Build a minimal OpenAPI doc for tests using the health handler
#[derive(OpenApi)]
#[openapi(paths(api::handlers::health::health_check))]
struct TestApiDoc;

#[tokio::test]
async fn test_swagger_ui_serves_html() {
    let app = Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", TestApiDoc::openapi()));

    let server = TestServer::new(app).unwrap();

    // SwaggerUi issues a redirect from /swagger-ui -> /swagger-ui/, so hit the trailing slash
    let resp = server.get("/swagger-ui/").await;
    resp.assert_status_ok();
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert!(ct.contains("text/html") || ct.starts_with("text/html"));
}

#[tokio::test]
async fn test_openapi_json_serves_json() {
    let app = Router::new()
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", TestApiDoc::openapi()));
    let server = TestServer::new(app).unwrap();

    let resp = server.get("/api-docs/openapi.json").await;
    resp.assert_status_ok();
    let ct = resp
        .headers()
        .get("content-type")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    assert!(ct.starts_with("application/json"));
    // Basic shape sanity
    let body: serde_json::Value = resp.json();
    assert!(body.get("paths").is_some());
}
