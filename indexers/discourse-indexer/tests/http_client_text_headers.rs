use discourse_indexer::discourse_api::DiscourseClient;
use mockito::Server;
use reqwest::header::{HeaderMap, HeaderValue};

/// Text endpoint propagates request headers and returns response headers/body.
#[tokio::test]
async fn test_text_endpoint_propagates_headers_and_body() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    let mock = server
        .mock("GET", "/hello")
        .match_header("X-Custom", "abc")
        .with_status(200)
        .with_header("X-Test", "ok")
        .with_header("Content-Type", "text/plain")
        .with_body("hi")
        .expect(1)
        .create_async()
        .await;

    let client = DiscourseClient::new(base_url);
    let mut extra = HeaderMap::new();
    extra.insert("X-Custom", HeaderValue::from_static("abc"));

    let (status, body, headers) = client
        .get_text_with_headers("/hello", &extra)
        .await
        .expect("text request");

    assert_eq!(status, reqwest::StatusCode::OK);
    assert_eq!(body, "hi");
    assert_eq!(headers.get("X-Test").unwrap().to_str().unwrap(), "ok");

    mock.assert_async().await;
}

/// Text endpoint returns status/body even on non-200 responses.
#[tokio::test]
async fn test_text_endpoint_returns_status_and_body_on_error() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    let mock = server
        .mock("GET", "/missing")
        .with_status(404)
        .with_header("Content-Type", "text/plain")
        .with_body("nf")
        .expect(1)
        .create_async()
        .await;

    let client = DiscourseClient::new(base_url);
    let (status, body, _headers) = client
        .get_text_with_headers("/missing", &HeaderMap::new())
        .await
        .expect("text request should return status even on 404");

    assert_eq!(status, reqwest::StatusCode::NOT_FOUND);
    assert_eq!(body, "nf");

    mock.assert_async().await;
}
