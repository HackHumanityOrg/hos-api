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

#[tokio::test]
async fn discourse_routes_are_wired_under_api_v1() {
    support::init_test_tracing();

    let response = support::build_router_with_state(None)
        .into_service()
        .oneshot(
            Request::get("/api/v1/discourse/instances")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn venear_routes_are_wired_under_api_v1() {
    support::init_test_tracing();

    let response = support::build_router_with_state(None)
        .into_service()
        .oneshot(
            Request::get("/api/v1/venear/proposals")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn venear_config_routes_are_wired_under_api_v1() {
    support::init_test_tracing();

    let response = support::build_router_with_state(None)
        .into_service()
        .oneshot(
            Request::get("/api/v1/venear/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn agora_routes_are_wired_under_api_namespace() {
    support::init_test_tracing();

    let response = support::build_router_with_state(None)
        .into_service()
        .oneshot(
            Request::get("/api/agora/proposal/approved")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn agora_delegate_routes_are_wired_under_api_namespace() {
    support::init_test_tracing();

    let response = support::build_router_with_state(None)
        .into_service()
        .oneshot(
            Request::get("/api/agora/delegates")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn telegram_routes_are_wired_under_api_v1() {
    support::init_test_tracing();

    let response = support::build_router_with_state(None)
        .into_service()
        .oneshot(
            Request::get("/api/v1/telegram/channels")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn identity_routes_are_wired_under_api_v1() {
    support::init_test_tracing();

    let response = support::build_router_with_state(None)
        .into_service()
        .oneshot(
            Request::get("/api/v1/identity/persons")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
}
