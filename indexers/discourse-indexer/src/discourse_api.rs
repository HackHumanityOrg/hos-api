use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use rand::RngExt;
use reqwest::{header::HeaderMap, Client, StatusCode};
use serde::de::DeserializeOwned;
use serde_path_to_error as spte;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::Semaphore;
use tracing::{debug, error, warn};

#[derive(Debug)]
pub struct HttpStatusError {
    pub status: StatusCode,
    pub url: String,
}

impl std::fmt::Display for HttpStatusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unexpected status {} for {}", self.status, self.url)
    }
}

impl std::error::Error for HttpStatusError {}

#[derive(Debug)]
pub struct MaxRetriesReachedError {
    pub status: StatusCode,
    pub url: String,
}

impl std::fmt::Display for MaxRetriesReachedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "max retries reached for {}: {}", self.url, self.status)
    }
}

impl std::error::Error for MaxRetriesReachedError {}

/// HTTP abstraction trait for testing and dependency injection
#[async_trait]
pub trait JsonFetcher: Send + Sync {
    async fn get_json<T: DeserializeOwned + Send + 'static>(&self, path: &str) -> Result<T>;
}

#[derive(Clone)]
pub struct DiscourseClient {
    base_url: String,
    http: Client,
    max_retries: usize,
    http_sem: Option<Arc<Semaphore>>,
    cache: Arc<Mutex<HashMap<String, (Instant, serde_json::Value)>>>,
    cache_ttl: Duration,
    cache_max_size: usize,
    retry_base_delay: Duration,
    jitter_min_ms: u64,
    jitter_max_ms: u64,
}

impl DiscourseClient {
    pub fn new(base_url: String) -> Self {
        let mut headers = HeaderMap::new();
        headers.insert(
            reqwest::header::USER_AGENT,
            reqwest::header::HeaderValue::from_static(
                "data-commons-discourse-indexer/0.1 (reqwest)",
            ),
        );
        let http = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(60))
            .pool_idle_timeout(Duration::from_secs(300)) // 5 minutes - good for DNS refresh
            .build()
            .expect("failed to build http client");

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
            max_retries: 10, // Increased for better persistence
            http_sem: Some(Arc::new(Semaphore::new(10))), // Default client-side rate limiting
            cache: Arc::new(Mutex::new(HashMap::new())),
            cache_ttl: Duration::from_secs(5),
            cache_max_size: 1000,
            retry_base_delay: Duration::from_secs(2),
            jitter_min_ms: 100,
            jitter_max_ms: 500,
        }
    }

    pub fn with_http_max_concurrency(mut self, max_concurrency: usize) -> Self {
        self.set_http_max_concurrency(max_concurrency);
        self
    }

    pub fn set_http_max_concurrency(&mut self, max_concurrency: usize) {
        self.http_sem = Some(Arc::new(Semaphore::new(max_concurrency.max(1))));
    }

    /// Determines if an HTTP status code represents a permanent error that should not be retried
    fn is_permanent_error(&self, status: StatusCode) -> bool {
        match status {
            // Client errors that are permanent
            StatusCode::BAD_REQUEST |          // 400 - malformed request
            StatusCode::UNAUTHORIZED |         // 401 - auth required/invalid
            StatusCode::PAYMENT_REQUIRED |     // 402 - payment required
            StatusCode::NOT_FOUND |            // 404 - resource doesn't exist
            StatusCode::METHOD_NOT_ALLOWED |   // 405 - method not supported
            StatusCode::NOT_ACCEPTABLE |       // 406 - content negotiation failed
            StatusCode::PROXY_AUTHENTICATION_REQUIRED | // 407 - proxy auth required
            StatusCode::REQUEST_TIMEOUT |      // 408 - client took too long
            StatusCode::CONFLICT |             // 409 - resource conflict
            StatusCode::GONE |                 // 410 - resource permanently removed
            StatusCode::LENGTH_REQUIRED |      // 411 - content-length required
            StatusCode::PRECONDITION_FAILED |  // 412 - precondition not met
            StatusCode::PAYLOAD_TOO_LARGE |    // 413 - request too large
            StatusCode::URI_TOO_LONG |         // 414 - URI too long
            StatusCode::UNSUPPORTED_MEDIA_TYPE | // 415 - unsupported content type
            StatusCode::RANGE_NOT_SATISFIABLE | // 416 - range not satisfiable
            StatusCode::EXPECTATION_FAILED |   // 417 - expectation failed
            StatusCode::UNPROCESSABLE_ENTITY | // 422 - validation failed
            StatusCode::LOCKED |               // 423 - resource locked
            StatusCode::FAILED_DEPENDENCY |    // 424 - dependency failed
            StatusCode::UPGRADE_REQUIRED |     // 426 - protocol upgrade required
            StatusCode::PRECONDITION_REQUIRED | // 428 - precondition required
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE => true, // 431 - headers too large

            // These client errors should be retried as they might be transient
            StatusCode::TOO_MANY_REQUESTS => false,     // 429 - rate limited, definitely retry

            // All server errors should be retried as they're likely transient
            status if status.is_server_error() => false,

            // All other client errors should not be retried
            status if status.is_client_error() => true,

            // Success and redirect codes should not reach this function, but just in case
            _ => false,
        }
    }

    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        // In-memory cache check
        if let Some((ts, v)) = self
            .cache
            .lock()
            .ok()
            .and_then(|map| map.get(path).cloned())
        {
            if ts.elapsed() <= self.cache_ttl {
                debug!(path = path, age_ms = ts.elapsed().as_millis(), "Cache hit");
                // Path-aware conversion from cached value
                let s = match serde_json::to_string(&v) {
                    Ok(s) => s,
                    Err(e) => return Err(anyhow!(e).context("failed to stringify cached json")),
                };
                let mut de = serde_json::Deserializer::from_str(&s);
                return spte::deserialize::<_, T>(&mut de).map_err(|e| {
                    let path_str = e.path().to_string();
                    let snippet = if s.len() > 1000 {
                        format!("{}...", &s[..1000])
                    } else {
                        s.clone()
                    };
                    anyhow!(e).context(format!(
                        "invalid cached json body at path: {} for {} | snippet: {}",
                        path_str, path, snippet
                    ))
                });
            } else {
                debug!(
                    path = path,
                    age_ms = ts.elapsed().as_millis(),
                    ttl_ms = self.cache_ttl.as_millis(),
                    "Cache expired"
                );
            }
        } else {
            debug!(path = path, "Cache miss");
        }

        let v: serde_json::Value = self
            .fetch_json::<serde_json::Value>(path, &HeaderMap::new())
            .await?;
        // store in cache with cleanup
        if let Ok(mut map) = self.cache.lock() {
            // Clean expired entries if cache is getting large
            if map.len() >= self.cache_max_size {
                let now = Instant::now();
                map.retain(|_, (ts, _)| now.duration_since(*ts) <= self.cache_ttl);

                // If still too large after cleanup, remove oldest entries
                if map.len() >= self.cache_max_size {
                    let mut entries: Vec<_> =
                        map.iter().map(|(k, (ts, _))| (k.clone(), *ts)).collect();
                    entries.sort_by_key(|(_, ts)| *ts);
                    let to_remove = entries.len().saturating_sub(self.cache_max_size / 2);
                    for (key, _) in entries.into_iter().take(to_remove) {
                        map.remove(&key);
                    }
                }
            }
            map.insert(path.to_string(), (Instant::now(), v.clone()));
        }
        // Path-aware conversion to T with detailed context
        let s =
            serde_json::to_string(&v).unwrap_or_else(|_| String::from("<unstringifyable-json>"));
        let mut de = serde_json::Deserializer::from_str(&s);
        spte::deserialize::<_, T>(&mut de).map_err(|e| {
            let path_str = e.path().to_string();
            let snippet = if s.len() > 1000 { format!("{}...", &s[..1000]) } else { s.clone() };
            error!(url = %format!("{}{}", self.base_url, path), deser_path = %path_str, "json deserialization failed");
            anyhow!(e).context(format!(
                "invalid json body at path: {} for {} | snippet: {}",
                path_str, path, snippet
            ))
        })
    }

    async fn fetch_json<T: DeserializeOwned>(&self, path: &str, extra: &HeaderMap) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let start = Instant::now();
        let mut delay = self.retry_base_delay;
        let mut attempt = 0usize;

        loop {
            attempt += 1;
            let request_start = Instant::now();

            // Concurrency gating if configured
            let _permit = match &self.http_sem {
                Some(sem) => {
                    debug!(path = path, "Acquiring HTTP semaphore permit");
                    Some(sem.acquire().await.expect("http semaphore closed"))
                }
                None => None,
            };
            let mut req = self.http.get(&url);
            if !extra.is_empty() {
                req = req.headers(extra.clone());
            }
            let resp = req.send().await.context("http request failed")?;

            // Basic rate signal handling
            if let (Some(rem), Some(reset)) = (
                resp.headers().get("X-RateLimit-Remaining"),
                resp.headers().get("X-RateLimit-Reset"),
            ) {
                let remaining = rem.to_str().ok().and_then(|s| s.parse::<i64>().ok());
                debug!(
                    path = path,
                    rate_limit_remaining = remaining,
                    "Rate limit info"
                );

                if remaining == Some(0) {
                    if let Some(reset_secs) =
                        reset.to_str().ok().and_then(|s| s.parse::<u64>().ok())
                    {
                        let now = SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        if reset_secs > now {
                            let sleep_for = Duration::from_secs(reset_secs - now + 1);
                            warn!(
                                path = path,
                                sleep_secs = sleep_for.as_secs(),
                                reset_at = reset_secs,
                                "Rate limit hit, sleeping until reset"
                            );
                            tokio::time::sleep(sleep_for).await;
                        }
                    }
                }
            }

            let status = resp.status();
            if status == StatusCode::OK {
                let v = resp.json::<T>().await.context("invalid json body")?;
                debug!(
                    path = path,
                    attempt = attempt,
                    request_ms = request_start.elapsed().as_millis(),
                    total_ms = start.elapsed().as_millis(),
                    "Request successful"
                );
                return Ok(v);
            }
            if status == StatusCode::NOT_MODIFIED {
                return Err(anyhow!(HttpStatusError {
                    status,
                    url: url.clone()
                }));
            }
            // Check if this is a permanent error that we should not retry
            if self.is_permanent_error(status) {
                if status == StatusCode::NOT_FOUND {
                    warn!(
                        path = path,
                        status = %status,
                        attempt = attempt,
                        total_ms = start.elapsed().as_millis(),
                        "Resource not found - not retrying"
                    );
                } else {
                    error!(
                        path = path,
                        status = %status,
                        attempt = attempt,
                        total_ms = start.elapsed().as_millis(),
                        "Permanent error - not retrying"
                    );
                }
                return Err(anyhow!(HttpStatusError {
                    status,
                    url: url.clone()
                }));
            }

            // All other errors are potentially transient and should be retried
            if attempt > self.max_retries {
                error!(
                    path = path,
                    status = %status,
                    attempt = attempt,
                    max_retries = self.max_retries,
                    total_ms = start.elapsed().as_millis(),
                    "Max retries reached"
                );
                return Err(anyhow!(MaxRetriesReachedError {
                    status,
                    url: url.clone()
                }));
            }

            // Retry-After seconds or HTTP-date
            let mut retry_hint = None;
            if let Some(ra) = resp.headers().get(reqwest::header::RETRY_AFTER) {
                if let Ok(s) = ra.to_str() {
                    if let Ok(secs) = s.parse::<u64>() {
                        retry_hint = Some(Duration::from_secs(secs));
                    } else if let Ok(dt) = httpdate::parse_http_date(s) {
                        let now = SystemTime::now();
                        if let Ok(d) = dt.duration_since(now) {
                            retry_hint = Some(d);
                        }
                    }
                }
            }
            let jitter: u64 = rand::rng().random_range(self.jitter_min_ms..=self.jitter_max_ms);

            if let Some(hint) = retry_hint {
                // Use the server's explicit retry hint with just jitter
                delay = hint + Duration::from_millis(jitter);
            } else {
                // Apply exponential backoff with jitter, but more aggressive for rate limits
                let backoff_multiplier = if status == StatusCode::TOO_MANY_REQUESTS {
                    1.5
                } else {
                    2.0
                };
                delay = std::cmp::min(
                    Duration::from_millis((delay.as_millis() as f64 * backoff_multiplier) as u64),
                    Duration::from_secs(120), // Increased max delay for persistent retries
                ) + Duration::from_millis(jitter);
            }

            warn!(
                path = path,
                status = %status,
                attempt = attempt,
                max_retries = self.max_retries,
                delay_ms = delay.as_millis(),
                retry_hint_ms = retry_hint.map(|d| d.as_millis()),
                jitter_ms = jitter,
                "Request failed, retrying"
            );

            tokio::time::sleep(delay).await;
            continue;
        }
    }
}

/// Implementation of JsonFetcher trait for DiscourseClient
#[async_trait]
impl JsonFetcher for DiscourseClient {
    async fn get_json<T: DeserializeOwned + Send + 'static>(&self, path: &str) -> Result<T> {
        DiscourseClient::get_json(self, path).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use serde::Deserialize;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Payload {
        value: i32,
    }

    fn test_client(base_url: String) -> DiscourseClient {
        let mut client = DiscourseClient::new(base_url);
        client.max_retries = 1;
        client.retry_base_delay = Duration::from_millis(1);
        client.jitter_min_ms = 0;
        client.jitter_max_ms = 0;
        client.cache_ttl = Duration::from_secs(60);
        client
    }

    #[test]
    fn classifies_permanent_status_codes() {
        let client = DiscourseClient::new("https://example.com".to_string());

        let permanent = [
            StatusCode::BAD_REQUEST,
            StatusCode::UNAUTHORIZED,
            StatusCode::PAYMENT_REQUIRED,
            StatusCode::NOT_FOUND,
            StatusCode::METHOD_NOT_ALLOWED,
            StatusCode::NOT_ACCEPTABLE,
            StatusCode::PROXY_AUTHENTICATION_REQUIRED,
            StatusCode::REQUEST_TIMEOUT,
            StatusCode::CONFLICT,
            StatusCode::GONE,
            StatusCode::LENGTH_REQUIRED,
            StatusCode::PRECONDITION_FAILED,
            StatusCode::PAYLOAD_TOO_LARGE,
            StatusCode::URI_TOO_LONG,
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            StatusCode::RANGE_NOT_SATISFIABLE,
            StatusCode::EXPECTATION_FAILED,
            StatusCode::UNPROCESSABLE_ENTITY,
            StatusCode::LOCKED,
            StatusCode::FAILED_DEPENDENCY,
            StatusCode::UPGRADE_REQUIRED,
            StatusCode::PRECONDITION_REQUIRED,
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE,
            StatusCode::IM_A_TEAPOT,
        ];
        for status in permanent {
            assert!(
                client.is_permanent_error(status),
                "{status} should be permanent"
            );
        }

        assert!(!client.is_permanent_error(StatusCode::TOO_MANY_REQUESTS));
        assert!(!client.is_permanent_error(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(!client.is_permanent_error(StatusCode::OK));
        assert!(!client.is_permanent_error(StatusCode::MOVED_PERMANENTLY));
    }

    #[tokio::test]
    async fn caches_responses_within_ttl() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/payload")
            .with_status(200)
            .with_body(r#"{"value":42}"#)
            .expect(1)
            .create_async()
            .await;

        let client = test_client(server.url());

        let first: Payload = client.get_json("/payload").await.expect("first payload");
        let second: Payload = client.get_json("/payload").await.expect("second payload");

        assert_eq!(first, Payload { value: 42 });
        assert_eq!(second, first);
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn returns_status_error_for_not_found() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/missing")
            .with_status(404)
            .create_async()
            .await;

        let client = test_client(server.url());

        let err = client
            .get_json::<Payload>("/missing")
            .await
            .expect_err("404 should fail");

        let status = err
            .chain()
            .find_map(|cause| cause.downcast_ref::<HttpStatusError>().map(|e| e.status));
        assert_eq!(status, Some(StatusCode::NOT_FOUND));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn returns_status_error_for_not_modified() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/not-modified")
            .with_status(304)
            .create_async()
            .await;

        let client = test_client(server.url());

        let err = client
            .get_json::<Payload>("/not-modified")
            .await
            .expect_err("304 should fail");

        let status = err
            .chain()
            .find_map(|cause| cause.downcast_ref::<HttpStatusError>().map(|e| e.status));
        assert_eq!(status, Some(StatusCode::NOT_MODIFIED));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn retries_transient_errors_until_max_retries() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/unstable")
            .with_status(503)
            .expect(2)
            .create_async()
            .await;

        let client = test_client(server.url());

        let err = client
            .get_json::<Payload>("/unstable")
            .await
            .expect_err("503 should eventually fail");

        let status = err.chain().find_map(|cause| {
            cause
                .downcast_ref::<MaxRetriesReachedError>()
                .map(|e| e.status)
        });
        assert_eq!(status, Some(StatusCode::SERVICE_UNAVAILABLE));
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn includes_json_path_context_on_deserialization_failure() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/bad-json")
            .with_status(200)
            .with_body(r#"{"value":"not-a-number"}"#)
            .create_async()
            .await;

        let client = test_client(server.url());
        let err = client
            .get_json::<Payload>("/bad-json")
            .await
            .expect_err("invalid payload should fail");

        let rendered = format!("{err:#}");
        assert!(rendered.contains("invalid json body at path"));
        assert!(rendered.contains("/bad-json"));
        mock.assert_async().await;
    }
}
