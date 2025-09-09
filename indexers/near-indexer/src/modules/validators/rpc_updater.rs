use anyhow::{Context, Result};
use common::config::{RateLimitConfig, RetryConfig};
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{policies::ExponentialBackoff, RetryTransientMiddleware};
use reqwest_tracing::TracingMiddleware;
use rust_decimal::Decimal;
use serde_json;
use std::time::Duration;
use tracing::{debug, warn};

/// RPC client for querying NEAR staking pool data with rate limiting and historical support
pub struct StakingPoolRpcUpdater {
    rpc_url: String,
    client: ClientWithMiddleware,
}

#[derive(Debug, Clone)]
pub struct HistoricalQuery {
    pub block_height: Option<u64>,
    pub block_hash: Option<String>,
}

/// JSON RPC 2.0 request structure
#[derive(Debug, serde::Serialize)]
struct JsonRpcRequest<T> {
    jsonrpc: String,
    id: String,
    method: String,
    params: T,
}

impl<T> JsonRpcRequest<T> {
    fn new(method: String, params: T) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id: "dontcare".to_string(),
            method,
            params,
        }
    }
}

/// JSON RPC 2.0 response structure
#[derive(Debug, serde::Deserialize)]
struct JsonRpcResponse<T> {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: String,
    #[serde(flatten)]
    result: JsonRpcResult<T>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum JsonRpcResult<T> {
    Ok { result: T },
    Err { error: NearRpcError },
}

/// NEAR RPC Error following the official verror convention
#[derive(Debug, serde::Deserialize)]
struct NearRpcError {
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    cause: Option<NearRpcErrorCause>,
    /// Legacy field, may be deprecated
    #[serde(default)]
    code: Option<i32>,
    /// Legacy field, may be deprecated
    #[serde(default)]
    message: Option<String>,
    /// Legacy field, may be deprecated
    #[serde(default)]
    #[allow(dead_code)]
    data: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct NearRpcErrorCause {
    #[allow(dead_code)]
    name: String,
    #[serde(default)]
    #[allow(dead_code)]
    info: Option<serde_json::Value>,
}

/// Query RPC method parameters for call_function
#[derive(Debug, serde::Serialize)]
struct CallFunctionRequest {
    request_type: String, // "call_function"
    #[serde(flatten)]
    block_reference: BlockReference,
    account_id: String,
    method_name: String,
    args_base64: String,
}

/// Block reference can be either finality or specific block
#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
enum BlockReference {
    Finality { finality: String },
    BlockId { block_id: BlockId },
}

/// Block ID can be either block height (number) or block hash (string)
#[derive(Debug, serde::Serialize)]
#[serde(untagged)]
enum BlockId {
    Height(u64),
    Hash(String),
}

/// Response structure for call_function queries
#[derive(Debug, serde::Deserialize)]
struct CallFunctionResponse {
    #[allow(dead_code)]
    block_hash: String,
    #[allow(dead_code)]
    block_height: u64,
    #[allow(dead_code)]
    logs: Vec<String>,
    result: Vec<u8>,
}

impl StakingPoolRpcUpdater {
    /// Create with default configuration (backwards compatibility)
    pub fn new(rpc_url: String, _update_interval_minutes: u64) -> Self {
        Self::with_config(rpc_url, RetryConfig::default(), RateLimitConfig::default())
    }

    /// Create with custom rate limiting (backwards compatibility)
    pub fn with_rate_limit(
        rpc_url: String,
        _update_interval_minutes: u64,
        _requests_per_second: u32,
    ) -> Self {
        // Rate limiting is now handled by the middleware
        Self::with_config(rpc_url, RetryConfig::default(), RateLimitConfig::default())
    }

    /// Create with full configuration
    pub fn with_config(
        rpc_url: String,
        retry_config: RetryConfig,
        _rate_limit_config: RateLimitConfig,
    ) -> Self {
        // Create retry policy based on configuration
        let retry_policy = ExponentialBackoff::builder()
            .retry_bounds(
                Duration::from_millis(retry_config.initial_backoff_ms),
                Duration::from_millis(retry_config.max_backoff_ms),
            )
            .build_with_max_retries(retry_config.max_retries);

        // Build client with middleware
        let client = ClientBuilder::new(reqwest::Client::new())
            .with(TracingMiddleware::default())
            .with(RetryTransientMiddleware::new_with_policy(retry_policy))
            .build();

        Self { rpc_url, client }
    }

    /// Query total staked balance at specific block
    pub async fn query_total_staked_balance_at(
        &self,
        pool_id: &str,
        query: Option<&HistoricalQuery>,
    ) -> Result<Decimal> {
        let block_reference = match query {
            Some(historical_query) => {
                if let Some(block_height) = historical_query.block_height {
                    BlockReference::BlockId {
                        block_id: BlockId::Height(block_height),
                    }
                } else if let Some(block_hash) = &historical_query.block_hash {
                    BlockReference::BlockId {
                        block_id: BlockId::Hash(block_hash.clone()),
                    }
                } else {
                    BlockReference::Finality {
                        finality: "final".to_string(),
                    }
                }
            }
            None => BlockReference::Finality {
                finality: "final".to_string(),
            },
        };

        let params = CallFunctionRequest {
            request_type: "call_function".to_string(),
            block_reference,
            account_id: pool_id.to_string(),
            method_name: "get_total_staked_balance".to_string(),
            args_base64: "".to_string(),
        };

        let response = self.call_view_method(params).await?;

        // Decode the bytes to a string
        let response_str = String::from_utf8(response.result)
            .context("Failed to decode RPC response bytes to UTF-8")?;

        // Parse the quoted string (remove surrounding quotes)
        let balance_str = response_str.trim_matches('"');
        let amount_yocto: u128 = balance_str
            .parse()
            .context("Failed to parse total staked balance from string")?;

        Ok(Self::yocto_to_near_decimal(amount_yocto))
    }

    /// Query commission rate at specific block
    pub async fn query_commission_rate_at(
        &self,
        pool_id: &str,
        query: Option<&HistoricalQuery>,
    ) -> Result<Decimal> {
        let block_reference = match query {
            Some(historical_query) => {
                if let Some(block_height) = historical_query.block_height {
                    BlockReference::BlockId {
                        block_id: BlockId::Height(block_height),
                    }
                } else if let Some(block_hash) = &historical_query.block_hash {
                    BlockReference::BlockId {
                        block_id: BlockId::Hash(block_hash.clone()),
                    }
                } else {
                    BlockReference::Finality {
                        finality: "final".to_string(),
                    }
                }
            }
            None => BlockReference::Finality {
                finality: "final".to_string(),
            },
        };

        let params = CallFunctionRequest {
            request_type: "call_function".to_string(),
            block_reference,
            account_id: pool_id.to_string(),
            method_name: "get_reward_fee_fraction".to_string(),
            args_base64: "".to_string(),
        };

        match self.call_view_method(params).await {
            Ok(response) => {
                // Decode the bytes to a string
                let response_str = String::from_utf8(response.result)
                    .context("Failed to decode commission rate response bytes to UTF-8")?;

                // Parse the JSON string
                let fraction: serde_json::Value = serde_json::from_str(&response_str)
                    .context("Failed to parse commission rate JSON")?;

                // Parse reward fee fraction: {"numerator": 1, "denominator": 100} = 1%
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

    /// Make a simple view method call using middleware client (with automatic retries)
    async fn call_view_method(&self, params: CallFunctionRequest) -> Result<CallFunctionResponse> {
        debug!("Making RPC request to {}", self.rpc_url);

        // Create JSON RPC request
        let request = JsonRpcRequest::new("query".to_string(), params);

        // Serialize request to JSON
        let json_body =
            serde_json::to_string(&request).context("Failed to serialize JSON RPC request")?;

        // Make request using middleware client (automatically handles retries, tracing, etc.)
        let response = self
            .client
            .post(&self.rpc_url)
            .timeout(Duration::from_secs(30))
            .header("content-type", "application/json")
            .body(json_body)
            .send()
            .await
            .context("Failed to send RPC request")?;

        // Parse response
        let response_text = response
            .text()
            .await
            .context("Failed to read RPC response")?;

        let rpc_response: JsonRpcResponse<CallFunctionResponse> =
            serde_json::from_str(&response_text).context("Failed to parse RPC response")?;

        match rpc_response.result {
            JsonRpcResult::Ok { result } => {
                debug!("RPC request succeeded");
                Ok(result)
            }
            JsonRpcResult::Err { error } => {
                // Create appropriate error
                let error_message = format!(
                    "NEAR RPC error '{}': {}",
                    error.name,
                    error.message.as_deref().unwrap_or("No message")
                );

                if error.code == Some(-429)
                    || error.name.contains("TIMEOUT")
                    || error
                        .message
                        .as_ref()
                        .is_some_and(|m| m.contains("Rate limit"))
                {
                    warn!("Rate limit error: {}", error_message);
                } else {
                    warn!("RPC error: {}", error_message);
                }

                Err(anyhow::anyhow!(error_message))
            }
        }
    }

    /// Get current rate limit status for monitoring purposes (stub - middleware handles this)
    pub async fn get_rate_limit_status(&self) -> Option<(u64, u64, Option<u64>)> {
        // The middleware handles rate limiting automatically, so we return None
        // to indicate that rate limit info is not available through this API anymore
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
