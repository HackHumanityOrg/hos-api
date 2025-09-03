use discourse_indexer::discourse_api::DiscourseClient;
use httpdate::HttpDate;
use mockito::Server;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::Instant;

/// When remaining=0 and reset is in the future, the client waits until reset.
#[tokio::test]
async fn test_respects_rate_limit_reset_header_when_remaining_zero() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let reset_time = now + 2; // Reset in 2 seconds

    let mock_rate_limited = server
        .mock("GET", "/test.json")
        .with_status(200)
        .with_header("X-RateLimit-Remaining", "0")
        .with_header("X-RateLimit-Reset", &reset_time.to_string())
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"success": true}"#)
        .expect(1)
        .create_async()
        .await;

    // Use small backoff to keep test fast
    let client = DiscourseClient::new_with_limits_and_backoff(
        base_url,
        5,
        None,
        std::time::Duration::from_millis(50),
        0,
        0,
    );
    let start = Instant::now();

    let result: serde_json::Value = client.get_json("/test.json").await.unwrap();

    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_secs(1)); // Should have waited
    assert_eq!(result["success"], true);

    mock_rate_limited.assert_async().await;
}

/// When remaining>0, the client proceeds without waiting.
#[tokio::test]
async fn test_does_not_wait_when_remaining_nonzero() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    let mock_normal = server
        .mock("GET", "/test.json")
        .with_status(200)
        .with_header("X-RateLimit-Remaining", "10")
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"success": true}"#)
        .expect(1)
        .create_async()
        .await;

    let client = DiscourseClient::new(base_url);
    let start = Instant::now();

    let result: serde_json::Value = client.get_json("/test.json").await.unwrap();

    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_millis(500)); // Should not have waited significantly
    assert_eq!(result["success"], true);

    mock_normal.assert_async().await;
}

/// 429 triggers retries with backoff and eventually succeeds.
#[tokio::test]
async fn test_retries_on_429_with_backoff() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    // First two requests return 429, third succeeds
    let mock_429_1 = server
        .mock("GET", "/test.json")
        .with_status(429)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"error": "rate limited"}"#)
        .expect(1)
        .create_async()
        .await;

    let mock_429_2 = server
        .mock("GET", "/test.json")
        .with_status(429)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"error": "rate limited"}"#)
        .expect(1)
        .create_async()
        .await;

    let mock_success = server
        .mock("GET", "/test.json")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"success": true}"#)
        .expect(1)
        .create_async()
        .await;

    let client = DiscourseClient::new(base_url);
    let start = Instant::now();

    let result: serde_json::Value = client.get_json("/test.json").await.unwrap();

    let elapsed = start.elapsed();
    // Should have waited for at least two retry delays (~100ms + 200ms)
    assert!(elapsed >= Duration::from_millis(250));
    assert_eq!(result["success"], true);

    mock_429_1.assert_async().await;
    mock_429_2.assert_async().await;
    mock_success.assert_async().await;
}

/// Retry-After seconds value is honored.
#[tokio::test]
async fn test_respects_retry_after_seconds() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    let mock_429 = server
        .mock("GET", "/test.json")
        .with_status(429)
        .with_header("Retry-After", "3") // 3 seconds
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"error": "rate limited"}"#)
        .expect(1)
        .create_async()
        .await;

    let mock_success = server
        .mock("GET", "/test.json")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"success": true}"#)
        .expect(1)
        .create_async()
        .await;

    let client = DiscourseClient::new(base_url);
    let start = Instant::now();

    let result: serde_json::Value = client.get_json("/test.json").await.unwrap();

    let elapsed = start.elapsed();
    // Should have waited at least 3 seconds as specified in Retry-After
    assert!(elapsed >= Duration::from_secs(2));
    assert_eq!(result["success"], true);

    mock_429.assert_async().await;
    mock_success.assert_async().await;
}

/// Retry-After HTTP-date is honored.
#[tokio::test]
async fn test_respects_retry_after_http_date() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    // Create a future date (2 seconds from now)
    let future_time = SystemTime::now() + Duration::from_secs(2);
    let http_date = HttpDate::from(future_time).to_string();

    let mock_429 = server
        .mock("GET", "/test.json")
        .with_status(429)
        .with_header("Retry-After", &http_date)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"error": "rate limited"}"#)
        .expect(1)
        .create_async()
        .await;

    let mock_success = server
        .mock("GET", "/test.json")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"success": true}"#)
        .expect(1)
        .create_async()
        .await;

    let client = DiscourseClient::new(base_url);
    let start = Instant::now();

    let result: serde_json::Value = client.get_json("/test.json").await.unwrap();

    let elapsed = start.elapsed();
    // Should have waited approximately 2 seconds
    assert!(elapsed >= Duration::from_secs(1));
    assert_eq!(result["success"], true);

    mock_429.assert_async().await;
    mock_success.assert_async().await;
}

/// After max retries, an error is returned.
#[tokio::test]
async fn test_returns_error_after_max_retries() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    // Return 429 for all attempts (more than max_retries which is 2 for this test)
    let mock_429 = server
        .mock("GET", "/test.json")
        .with_status(429)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"error": "rate limited"}"#)
        .expect(3) // Initial attempt + 2 retries
        .create_async()
        .await;

    let client = DiscourseClient::new_with_retries(base_url, 2);

    let result = client.get_json::<serde_json::Value>("/test.json").await;

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("max retries reached"));

    mock_429.assert_async().await;
}

/// 5xx errors are retried.
#[tokio::test]
async fn test_retries_on_500_then_succeeds() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    let mock_500 = server
        .mock("GET", "/test.json")
        .with_status(500)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"error": "internal server error"}"#)
        .expect(1)
        .create_async()
        .await;

    let mock_success = server
        .mock("GET", "/test.json")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"success": true}"#)
        .expect(1)
        .create_async()
        .await;

    let client = DiscourseClient::new(base_url);

    let result: serde_json::Value = client.get_json("/test.json").await.unwrap();

    assert_eq!(result["success"], true);

    mock_500.assert_async().await;
    mock_success.assert_async().await;
}

/// 4xx client errors are not retried.
#[tokio::test]
async fn test_fails_immediately_on_400() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    let mock_400 = server
        .mock("GET", "/test.json")
        .with_status(400)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"error": "bad request"}"#)
        .expect(1)
        .create_async()
        .await;

    let client = DiscourseClient::new(base_url);

    let result = client.get_json::<serde_json::Value>("/test.json").await;

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("unexpected status 400"));

    mock_400.assert_async().await;
}

/// 304 is surfaced as an error.
#[tokio::test]
async fn test_returns_error_on_304_not_modified() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    let mock_304 = server
        .mock("GET", "/test.json")
        .with_status(304)
        .expect(1)
        .create_async()
        .await;

    let client = DiscourseClient::new(base_url);

    let result = client.get_json::<serde_json::Value>("/test.json").await;

    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("304 Not Modified"));

    mock_304.assert_async().await;
}

/// 200 OK returns quickly with JSON body.
#[tokio::test]
async fn test_successful_200_no_delay() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    let mock_success = server
        .mock("GET", "/test.json")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"data": "test"}"#)
        .expect(1)
        .create_async()
        .await;

    let client = DiscourseClient::new(base_url);
    let start = Instant::now();

    let result: serde_json::Value = client.get_json("/test.json").await.unwrap();

    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_millis(100)); // Should be very fast
    assert_eq!(result["data"], "test");

    mock_success.assert_async().await;
}

/// If remaining=0 but reset header is missing, do not wait.
#[tokio::test]
async fn test_no_wait_when_remaining_zero_without_reset_header() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    let mock = server
        .mock("GET", "/test.json")
        .with_status(200)
        .with_header("X-RateLimit-Remaining", "0")
        // Intentionally omit X-RateLimit-Reset to verify no sleep fallback
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"ok": true}"#)
        .expect(1)
        .create_async()
        .await;

    let client = DiscourseClient::new(base_url);
    let start = Instant::now();

    let result: serde_json::Value = client.get_json("/test.json").await.unwrap();

    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_millis(500));
    assert_eq!(result["ok"], true);

    mock.assert_async().await;
}

/// If reset is in the past, do not wait.
#[tokio::test]
async fn test_no_wait_when_reset_in_past() {
    let mut server = Server::new_async().await;
    let base_url = server.url();

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let past_reset = now.saturating_sub(30);

    let mock = server
        .mock("GET", "/test.json")
        .with_status(200)
        .with_header("X-RateLimit-Remaining", "0")
        .with_header("X-RateLimit-Reset", &past_reset.to_string())
        .with_header("Content-Type", "application/json")
        .with_body(r#"{"ok": true}"#)
        .expect(1)
        .create_async()
        .await;

    let client = DiscourseClient::new(base_url);
    let start = Instant::now();

    let result: serde_json::Value = client.get_json("/test.json").await.unwrap();

    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_millis(500));
    assert_eq!(result["ok"], true);

    mock.assert_async().await;
}
