use anyhow::{anyhow, Context, Result};
use rand::Rng;
use reqwest::{header::HeaderMap, Client, StatusCode};
use serde::de::DeserializeOwned;
use serde_path_to_error as spte;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::Semaphore;
use tracing::error;

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

#[derive(Clone)]
pub struct DiscourseClient {
    base_url: String,
    http: Client,
    max_retries: usize,
    http_sem: Option<Arc<Semaphore>>,
    cache: Arc<Mutex<HashMap<String, (Instant, serde_json::Value)>>>,
    cache_ttl: Duration,
    retry_base_delay: Duration,
    jitter_min_ms: u64,
    jitter_max_ms: u64,
}

impl DiscourseClient {
    pub fn new(base_url: String) -> Self {
        Self::new_with_limits(base_url, 5, None)
    }

    pub fn new_with_retries(base_url: String, max_retries: usize) -> Self {
        Self::new_with_limits(base_url, max_retries, None)
    }

    pub fn new_with_limits(
        base_url: String,
        max_retries: usize,
        http_limit: Option<usize>,
    ) -> Self {
        Self::new_with_limits_and_backoff(
            base_url,
            max_retries,
            http_limit,
            Duration::from_secs(2), // default base delay
            100,
            500,
        )
    }

    pub fn new_with_limits_and_backoff(
        base_url: String,
        max_retries: usize,
        http_limit: Option<usize>,
        retry_base_delay: Duration,
        jitter_min_ms: u64,
        jitter_max_ms: u64,
    ) -> Self {
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
            .build()
            .expect("failed to build http client");

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
            max_retries,
            http_sem: http_limit.map(|n| Arc::new(Semaphore::new(n.max(1)))),
            cache: Arc::new(Mutex::new(HashMap::new())),
            cache_ttl: Duration::from_secs(5),
            retry_base_delay,
            jitter_min_ms,
            jitter_max_ms: if jitter_max_ms < jitter_min_ms {
                jitter_min_ms
            } else {
                jitter_max_ms
            },
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
            }
        }

        let v: serde_json::Value = self
            .get_json_with_headers::<serde_json::Value>(path, &HeaderMap::new())
            .await?;
        // store in cache
        if let Ok(mut map) = self.cache.lock() {
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

    pub async fn get_json_with_headers<T: DeserializeOwned>(
        &self,
        path: &str,
        extra: &HeaderMap,
    ) -> Result<T> {
        let url = format!("{}{}", self.base_url, path);
        let mut delay = self.retry_base_delay;
        let mut attempt = 0usize;
        loop {
            attempt += 1;
            // Concurrency gating if configured
            let _permit = match &self.http_sem {
                Some(sem) => Some(sem.acquire().await.expect("http semaphore closed")),
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
                if rem.to_str().ok().and_then(|s| s.parse::<i64>().ok()) == Some(0) {
                    if let Some(reset_secs) =
                        reset.to_str().ok().and_then(|s| s.parse::<u64>().ok())
                    {
                        let now = SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        if reset_secs > now {
                            let sleep_for = Duration::from_secs(reset_secs - now + 1);
                            tokio::time::sleep(sleep_for).await;
                        }
                    }
                }
            }

            let status = resp.status();
            if status == StatusCode::OK {
                let v = resp.json::<T>().await.context("invalid json body")?;
                return Ok(v);
            }
            if status == StatusCode::NOT_MODIFIED {
                return Err(anyhow!(HttpStatusError {
                    status,
                    url: url.clone()
                }));
            }
            if status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                if attempt > self.max_retries {
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
                let jitter: u64 =
                    rand::thread_rng().gen_range(self.jitter_min_ms..=self.jitter_max_ms);
                
                if let Some(hint) = retry_hint {
                    // Use the server's explicit retry hint with just jitter
                    delay = hint + Duration::from_millis(jitter);
                } else {
                    // Apply exponential backoff with jitter
                    delay = std::cmp::min(delay * 2, Duration::from_secs(60))
                        + Duration::from_millis(jitter);
                }
                tokio::time::sleep(delay).await;
                continue;
            }
            return Err(anyhow!(HttpStatusError {
                status,
                url: url.clone()
            }));
        }
    }

    pub async fn get_text_with_headers(
        &self,
        path: &str,
        extra: &HeaderMap,
    ) -> Result<(StatusCode, String, HeaderMap)> {
        let url = format!("{}{}", self.base_url, path);
        // Concurrency gating if configured
        let _permit = match &self.http_sem {
            Some(sem) => Some(sem.acquire().await.expect("http semaphore closed")),
            None => None,
        };
        let mut req = self.http.get(&url);
        if !extra.is_empty() {
            req = req.headers(extra.clone());
        }
        let resp = req.send().await.context("http request failed")?;
        let status = resp.status();
        let headers = resp.headers().clone();
        let body = resp.text().await.unwrap_or_default();
        Ok((status, body, headers))
    }
}
