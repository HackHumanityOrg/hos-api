use anyhow::{Context, Result};
use rust_decimal::Decimal;
use serde_json;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, warn};

/// RPC client for querying NEAR staking pool data with rate limiting and historical support
pub struct StakingPoolRpcUpdater {
    rpc_url: String,
    client: reqwest::Client,
    rate_limiter: Arc<Semaphore>,
    requests_per_second: u32,
    last_request_time: Arc<Mutex<Instant>>,
    /// Rate limit info from server headers
    rate_limit_info: Arc<Mutex<Option<RateLimitInfo>>>,
}

#[derive(Debug, Clone)]
struct RateLimitInfo {
    /// Number of requests remaining in current window
    remaining: Option<u64>,
    /// Total requests allowed per window
    limit: Option<u64>,
    /// Time when rate limit window resets (Unix timestamp)
    reset_time: Option<u64>,
    /// Duration until rate limit resets
    retry_after: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct HistoricalQuery {
    pub block_height: Option<u64>,
    pub block_hash: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct RpcResponse<T> {
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Debug, serde::Deserialize)]
struct RpcError {
    message: String,
    code: i32,
}

impl StakingPoolRpcUpdater {
    pub fn new(rpc_url: String, _update_interval_minutes: u64) -> Self {
        let requests_per_second = 5; // Conservative rate limit for archival nodes

        Self {
            rpc_url,
            client: reqwest::Client::new(),
            rate_limiter: Arc::new(Semaphore::new(requests_per_second as usize)),
            requests_per_second,
            last_request_time: Arc::new(Mutex::new(Instant::now())),
            rate_limit_info: Arc::new(Mutex::new(None)),
        }
    }

    /// Create with custom rate limiting
    pub fn with_rate_limit(
        rpc_url: String,
        _update_interval_minutes: u64,
        requests_per_second: u32,
    ) -> Self {
        Self {
            rpc_url,
            client: reqwest::Client::new(),
            rate_limiter: Arc::new(Semaphore::new(requests_per_second as usize)),
            requests_per_second,
            last_request_time: Arc::new(Mutex::new(Instant::now())),
            rate_limit_info: Arc::new(Mutex::new(None)),
        }
    }

    /// Query total staked balance at specific block
    pub async fn query_total_staked_balance_at(
        &self,
        pool_id: &str,
        query: Option<&HistoricalQuery>,
    ) -> Result<Decimal> {
        let mut params = serde_json::json!({
            "request_type": "call_function",
            "account_id": pool_id,
            "method_name": "get_total_staked_balance",
            "args_base64": ""
        });

        // Add block context - use block_id for historical queries or finality for current
        if let Some(historical_query) = query {
            if let Some(block_height) = historical_query.block_height {
                params["block_id"] =
                    serde_json::Value::Number(serde_json::Number::from(block_height));
            } else if let Some(block_hash) = &historical_query.block_hash {
                params["block_id"] = serde_json::Value::String(block_hash.clone());
            } else {
                params["finality"] = serde_json::Value::String("final".to_string());
            }
        } else {
            params["finality"] = serde_json::Value::String("final".to_string());
        }

        let response: String = self.call_view_method(params).await?;

        // Parse the returned amount (in yoctoNEAR)
        let amount_yocto: u128 = response
            .trim_matches('"')
            .parse()
            .context("Failed to parse total staked balance")?;

        Ok(Self::yocto_to_near_decimal(amount_yocto))
    }

    /// Query commission rate at specific block
    pub async fn query_commission_rate_at(
        &self,
        pool_id: &str,
        query: Option<&HistoricalQuery>,
    ) -> Result<Decimal> {
        let mut params = serde_json::json!({
            "request_type": "call_function",
            "account_id": pool_id,
            "method_name": "get_reward_fee_fraction",
            "args_base64": ""
        });

        // Add block context - use block_id for historical queries or finality for current
        if let Some(historical_query) = query {
            if let Some(block_height) = historical_query.block_height {
                params["block_id"] =
                    serde_json::Value::Number(serde_json::Number::from(block_height));
            } else if let Some(block_hash) = &historical_query.block_hash {
                params["block_id"] = serde_json::Value::String(block_hash.clone());
            } else {
                params["finality"] = serde_json::Value::String("final".to_string());
            }
        } else {
            params["finality"] = serde_json::Value::String("final".to_string());
        }

        match self.call_view_method::<serde_json::Value>(params).await {
            Ok(fraction) => {
                // Parse reward fee fraction: {"numerator": 5, "denominator": 100} = 5%
                let numerator = fraction
                    .get("numerator")
                    .and_then(|n| n.as_u64())
                    .unwrap_or(5) as u32;
                let denominator = fraction
                    .get("denominator")
                    .and_then(|d| d.as_u64())
                    .unwrap_or(100) as u32;

                let rate = Decimal::new(numerator as i64, 0) / Decimal::new(denominator as i64, 0);
                Ok(rate)
            }
            Err(e) => {
                warn!(error = %e, pool_id, "Failed to query commission rate, using default 5%");
                Ok(Decimal::new(5, 2)) // Default 5%
            }
        }
    }

    /// Make a rate-limited view method call to NEAR RPC with header-aware rate limiting
    async fn call_view_method<T>(&self, params: serde_json::Value) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        // Apply server-informed rate limiting first
        self.apply_server_rate_limiting().await?;

        // Acquire rate limiter permit
        let _permit = self
            .rate_limiter
            .acquire()
            .await
            .context("Rate limiter closed")?;

        // Enforce minimum time between requests (fallback)
        {
            let mut last_request = self.last_request_time.lock().await;
            let min_interval = Duration::from_secs(1) / self.requests_per_second;
            let elapsed = last_request.elapsed();

            if elapsed < min_interval {
                let sleep_duration = min_interval - elapsed;
                debug!("Rate limiting: sleeping for {:?}", sleep_duration);
                tokio::time::sleep(sleep_duration).await;
            }

            *last_request = Instant::now();
        }

        let request_body = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "dontcare",
            "method": "query",
            "params": params
        });

        debug!("Making RPC request to {}", self.rpc_url);

        let response = self
            .client
            .post(&self.rpc_url)
            .json(&request_body)
            .timeout(Duration::from_secs(30)) // 30 second timeout for archival queries
            .send()
            .await
            .context("Failed to send RPC request")?;

        // Extract rate limit information from headers before consuming response
        self.update_rate_limit_info(&response).await;

        let response_text = response
            .text()
            .await
            .context("Failed to read RPC response")?;

        let rpc_response: RpcResponse<T> =
            serde_json::from_str(&response_text).context("Failed to parse RPC response")?;

        if let Some(error) = rpc_response.error {
            return Err(anyhow::anyhow!(
                "RPC error {}: {}",
                error.code,
                error.message
            ));
        }

        rpc_response
            .result
            .ok_or_else(|| anyhow::anyhow!("RPC response missing result"))
    }

    /// Apply rate limiting based on server response headers
    async fn apply_server_rate_limiting(&self) -> Result<()> {
        let rate_info = self.rate_limit_info.lock().await;

        if let Some(info) = rate_info.as_ref() {
            // Check if we need to wait based on retry-after header
            if let Some(retry_after) = info.retry_after {
                debug!("Rate limit retry-after: waiting {:?}", retry_after);
                tokio::time::sleep(retry_after).await;
                return Ok(());
            }

            // Check if we're near the rate limit based on remaining requests
            if let (Some(remaining), Some(limit)) = (info.remaining, info.limit) {
                let usage_percentage = if limit > 0 {
                    (limit - remaining) as f64 / limit as f64 * 100.0
                } else {
                    100.0
                };

                if usage_percentage > 90.0 {
                    // We're using >90% of rate limit, slow down
                    let wait_time = if let Some(reset_time) = info.reset_time {
                        // Calculate time until reset
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();

                        if reset_time > now {
                            Duration::from_secs((reset_time - now) / (remaining + 1).max(1))
                        } else {
                            Duration::from_millis(200) // Default small delay
                        }
                    } else {
                        Duration::from_millis(500) // Conservative delay when approaching limit
                    };

                    debug!(
                        "Rate limit usage at {:.1}% ({}/{}), waiting {:?}",
                        usage_percentage,
                        limit - remaining,
                        limit,
                        wait_time
                    );
                    tokio::time::sleep(wait_time).await;
                }
            }
        }

        Ok(())
    }

    /// Update rate limit information from HTTP response headers
    async fn update_rate_limit_info(&self, response: &reqwest::Response) {
        let headers = response.headers();

        let rate_info = RateLimitInfo {
            // Common rate limit headers
            remaining: headers
                .get("x-ratelimit-remaining")
                .or_else(|| headers.get("x-rate-limit-remaining"))
                .or_else(|| headers.get("ratelimit-remaining"))
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok()),

            limit: headers
                .get("x-ratelimit-limit")
                .or_else(|| headers.get("x-rate-limit-limit"))
                .or_else(|| headers.get("ratelimit-limit"))
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok()),

            reset_time: headers
                .get("x-ratelimit-reset")
                .or_else(|| headers.get("x-rate-limit-reset"))
                .or_else(|| headers.get("ratelimit-reset"))
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse().ok()),

            retry_after: headers
                .get("retry-after")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .map(Duration::from_secs),
        };

        // Only update if we got meaningful information
        if rate_info.remaining.is_some()
            || rate_info.limit.is_some()
            || rate_info.reset_time.is_some()
            || rate_info.retry_after.is_some()
        {
            let mut info_guard = self.rate_limit_info.lock().await;
            *info_guard = Some(rate_info);

            debug!(
                "Updated rate limit info: remaining={:?}, limit={:?}, reset={:?}, retry_after={:?}",
                info_guard.as_ref().unwrap().remaining,
                info_guard.as_ref().unwrap().limit,
                info_guard.as_ref().unwrap().reset_time,
                info_guard.as_ref().unwrap().retry_after
            );
        }
    }

    /// Get current rate limit status for monitoring purposes
    pub async fn get_rate_limit_status(&self) -> Option<(u64, u64, Option<u64>)> {
        let rate_info = self.rate_limit_info.lock().await;

        if let Some(info) = rate_info.as_ref() {
            if let (Some(remaining), Some(limit)) = (info.remaining, info.limit) {
                return Some((remaining, limit, info.reset_time));
            }
        }

        None
    }

    /// Convert yoctoNEAR (10^-24 NEAR) to NEAR decimal
    fn yocto_to_near_decimal(yocto_amount: u128) -> Decimal {
        // 1 NEAR = 10^24 yoctoNEAR
        use rust_decimal::Decimal;
        use std::str::FromStr;
        
        let yocto_decimal = Decimal::from_str(&yocto_amount.to_string()).unwrap_or(Decimal::ZERO);
        let near_divisor = Decimal::from_str("1000000000000000000000000").unwrap(); // 10^24
        
        yocto_decimal / near_divisor
    }
}
