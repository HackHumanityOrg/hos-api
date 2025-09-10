use crate::modules::staking_pools::types::{DetectionMethod, PoolType};

/// Pool detection configuration
pub struct PoolDetector {
    pool_patterns: Vec<String>,
    custom_pools: Vec<String>,
}

impl PoolDetector {
    pub fn new(pool_patterns: Vec<String>, custom_pools: Vec<String>) -> Self {
        Self {
            pool_patterns,
            custom_pools,
        }
    }

    /// Detect if an account ID is a staking pool
    pub fn detect_pool(&self, account_id: &str) -> Option<(PoolType, DetectionMethod)> {
        // Check custom pools first
        if self.custom_pools.contains(&account_id.to_string()) {
            return Some((PoolType::Custom, DetectionMethod::Config));
        }

        // Check patterns
        for pattern in &self.pool_patterns {
            if self.matches_pattern(account_id, pattern) {
                let pool_type = self.pattern_to_pool_type(pattern);
                return Some((pool_type, DetectionMethod::Pattern));
            }
        }

        None
    }

    /// Check if an account receiving staking method calls might be a pool
    pub fn could_be_pool_by_method(&self, account_id: &str) -> bool {
        // Don't detect already known pools
        if self.detect_pool(account_id).is_some() {
            return false;
        }

        // Could be a custom pool if it receives staking methods
        true
    }

    /// Simple wildcard pattern matching
    fn matches_pattern(&self, account_id: &str, pattern: &str) -> bool {
        if pattern.starts_with('*') && pattern.ends_with("*") {
            let middle = &pattern[1..pattern.len()-1];
            account_id.contains(middle)
        } else if pattern.starts_with('*') {
            let suffix = &pattern[1..];
            account_id.ends_with(suffix)
        } else if pattern.ends_with('*') {
            let prefix = &pattern[..pattern.len()-1];
            account_id.starts_with(prefix)
        } else {
            account_id == pattern
        }
    }

    /// Convert pattern to pool type
    fn pattern_to_pool_type(&self, pattern: &str) -> PoolType {
        if pattern.contains("poolv1") {
            PoolType::PoolV1
        } else if pattern.contains("pool") {
            PoolType::Pool
        } else {
            PoolType::Custom
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_detection() {
        let detector = PoolDetector::new(
            vec!["*.poolv1.near".to_string(), "*.pool.near".to_string()],
            vec!["custom.staking".to_string()],
        );

        // Test pattern detection
        assert_eq!(
            detector.detect_pool("alice.poolv1.near"),
            Some((PoolType::PoolV1, DetectionMethod::Pattern))
        );
        assert_eq!(
            detector.detect_pool("bob.pool.near"),
            Some((PoolType::Pool, DetectionMethod::Pattern))
        );

        // Test custom pool detection
        assert_eq!(
            detector.detect_pool("custom.staking"),
            Some((PoolType::Custom, DetectionMethod::Config))
        );

        // Test non-pool
        assert_eq!(detector.detect_pool("alice.near"), None);
    }

    #[test]
    fn test_pattern_matching() {
        let detector = PoolDetector::new(vec![], vec![]);

        assert!(detector.matches_pattern("alice.poolv1.near", "*.poolv1.near"));
        assert!(detector.matches_pattern("bob.pool.near", "*.pool.near"));
        assert!(!detector.matches_pattern("charlie.near", "*.pool.near"));
    }
}