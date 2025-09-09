//! Database retry utilities for handling deadlocks and connection pool issues

use tracing::warn;

/// Retry a database operation if it fails due to a deadlock or connection pool issue.
/// Uses exponential backoff with jitter for transient database errors.
///
/// This function handles:
/// - MySQL deadlocks (errno 1213, sqlstate 40001)
/// - Connection pool timeouts
/// - Connection resets and other transient network issues
pub async fn with_db_retry<F, Fut, T, E>(mut op: F) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display + std::fmt::Debug,
{
    use rand::Rng;
    let mut attempts = 0u32;
    const MAX_ATTEMPTS: u32 = 10;
    const BASE_DELAY_MS: u64 = 100; // Start at 100ms
    const MAX_DELAY_MS: u64 = 5000; // Maximum 5 seconds

    loop {
        attempts += 1;
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                let error_str = format!("{}", e);
                let is_retryable_error = is_retryable_db_error(&error_str);

                if is_retryable_error && attempts <= MAX_ATTEMPTS {
                    // Exponential backoff: 100ms, 200ms, 400ms, 800ms, 1600ms, 3200ms, 5000ms max
                    let exponential_delay = BASE_DELAY_MS * (2u64.pow(attempts - 1));
                    let capped_delay = exponential_delay.min(MAX_DELAY_MS);

                    // Add jitter (±25% of delay)
                    let jitter_range = capped_delay / 4;
                    let jitter: u64 = rand::thread_rng().gen_range(0..=jitter_range);
                    let final_delay = capped_delay - jitter_range / 2 + jitter;

                    let error_type = classify_db_error(&error_str);

                    warn!(
                        attempts,
                        max_attempts = MAX_ATTEMPTS,
                        delay_ms = final_delay,
                        error_type,
                        error = %e,
                        "Retryable database error detected, using exponential backoff"
                    );

                    tokio::time::sleep(std::time::Duration::from_millis(final_delay)).await;
                    continue;
                }

                if is_retryable_error {
                    warn!(
                        attempts,
                        max_attempts = MAX_ATTEMPTS,
                        error = %e,
                        "Database operation failed after maximum retry attempts"
                    );
                }

                return Err(e);
            }
        }
    }
}

/// Classify the type of database error for logging purposes
fn classify_db_error(error_str: &str) -> &'static str {
    if error_str.contains("Deadlock found")
        || error_str.contains("errno 1213")
        || error_str.contains("sqlstate 40001")
    {
        "deadlock"
    } else if error_str.contains("Connection pool timed out")
        || error_str.contains("Failed to acquire connection from pool")
    {
        "connection_pool"
    } else if error_str.contains("Lost connection to MySQL server")
        || error_str.contains("Connection reset by peer")
        || error_str.contains("Broken pipe")
    {
        "connection_lost"
    } else {
        "other_transient"
    }
}

/// Determine if a database error is retryable
fn is_retryable_db_error(error_str: &str) -> bool {
    // Deadlock errors
    error_str.contains("Deadlock found") ||
    error_str.contains("errno 1213") ||
    error_str.contains("sqlstate 40001") ||
    // Connection pool errors
    error_str.contains("Connection pool timed out") ||
    error_str.contains("Failed to acquire connection from pool") ||
    // Connection issues
    error_str.contains("Lost connection to MySQL server") ||
    error_str.contains("Connection reset by peer") ||
    error_str.contains("Broken pipe") ||
    // PlanetScale specific errors
    error_str.contains("server has gone away") ||
    // Lock wait timeout (often transient)
    error_str.contains("Lock wait timeout exceeded")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_deadlock_error() {
        assert_eq!(
            classify_db_error("Deadlock found when trying to get lock"),
            "deadlock"
        );
        assert_eq!(classify_db_error("errno 1213"), "deadlock");
        assert_eq!(classify_db_error("sqlstate 40001"), "deadlock");
    }

    #[test]
    fn test_classify_connection_pool_error() {
        assert_eq!(
            classify_db_error("Connection pool timed out"),
            "connection_pool"
        );
        assert_eq!(
            classify_db_error("Failed to acquire connection from pool"),
            "connection_pool"
        );
    }

    #[test]
    fn test_is_retryable() {
        assert!(is_retryable_db_error(
            "Deadlock found when trying to get lock"
        ));
        assert!(is_retryable_db_error("Connection pool timed out"));
        assert!(is_retryable_db_error("Lost connection to MySQL server"));
        assert!(!is_retryable_db_error("Table 'users' doesn't exist"));
        assert!(!is_retryable_db_error("Syntax error"));
    }
}
