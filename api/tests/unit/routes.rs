use api::routes;
use api::state::AppState;
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
        .into_service()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn nested_routes_do_not_shadow_health_endpoint() {
    support::init_test_tracing();

    let response = routes::create_routes()
        .with_state(AppState::new(None))
        .into_service()
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
