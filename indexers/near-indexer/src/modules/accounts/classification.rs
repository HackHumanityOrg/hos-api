/// Account classification utilities for NEAR Protocol accounts
use tracing::trace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountType {
    System,
    Implicit,
    EthImplicit,
    Named,
    Lockup,
}

impl AccountType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccountType::System => "system",
            AccountType::Implicit => "implicit",
            AccountType::EthImplicit => "eth_implicit",
            AccountType::Named => "named",
            AccountType::Lockup => "lockup",
        }
    }
}

impl From<AccountType> for String {
    fn from(account_type: AccountType) -> Self {
        account_type.as_str().to_string()
    }
}

/// Classify account type based on account ID pattern according to NEAR Protocol rules
pub fn classify_account(account_id: &str) -> AccountType {
    let account_type = if account_id == "system" {
        AccountType::System
    } else if is_eth_implicit_account(account_id) {
        AccountType::EthImplicit
    } else if is_near_implicit_account(account_id) {
        AccountType::Implicit
    } else if account_id.contains("lockup") {
        AccountType::Lockup
    } else {
        AccountType::Named
    };

    trace!(
        account_id = %account_id,
        account_type = %account_type.as_str(),
        "Classified account type"
    );

    account_type
}

/// Check if account ID represents an ETH-implicit account
/// ETH-implicit accounts: 0x + 40 lowercase hex characters
pub fn is_eth_implicit_account(account_id: &str) -> bool {
    if !account_id.starts_with("0x") {
        return false;
    }

    if account_id.len() != 42 {
        return false;
    }

    let hex_part = &account_id[2..];
    let all_hex = hex_part.chars().all(|c| c.is_ascii_hexdigit());
    let all_lowercase = hex_part.chars().all(|c| !c.is_ascii_uppercase());

    all_hex && all_lowercase
}

/// Check if account ID represents a NEAR-implicit account
/// NEAR-implicit accounts: exactly 64 lowercase hex characters
pub fn is_near_implicit_account(account_id: &str) -> bool {
    if account_id.len() != 64 {
        return false;
    }

    let all_hex = account_id.chars().all(|c| c.is_ascii_hexdigit());
    let all_lowercase = account_id.chars().all(|c| !c.is_ascii_uppercase());

    all_hex && all_lowercase
}

/// Validate account ID format according to NEAR Protocol rules
pub fn validate_account_id(account_id: &str) -> Result<(), String> {
    // Account ID must be non-empty and at most 64 characters
    if account_id.is_empty() {
        return Err("Account ID cannot be empty".to_string());
    }

    if account_id.len() > 64 {
        return Err(format!(
            "Account ID cannot be longer than 64 characters, got {}",
            account_id.len()
        ));
    }

    // Implicit accounts (64 hex characters) - allow them
    if is_near_implicit_account(account_id) {
        return Ok(());
    }

    // ETH-implicit accounts (0x + 40 hex characters) - allow them
    if is_eth_implicit_account(account_id) {
        return Ok(());
    }

    // Named accounts - must contain only valid characters
    for (i, char) in account_id.chars().enumerate() {
        if !char.is_ascii_lowercase()
            && !char.is_ascii_digit()
            && char != '-'
            && char != '_'
            && char != '.'
        {
            return Err(format!(
                "Account ID contains invalid character '{}' at position {}",
                char, i
            ));
        }
    }

    Ok(())
}

/// Extract parent account ID for sub-accounts
pub fn get_parent_account(account_id: &str) -> Option<String> {
    if account_id.contains('.') {
        let parts: Vec<&str> = account_id.split('.').collect();
        if parts.len() > 1 {
            return Some(parts[1..].join("."));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_system_account() {
        assert_eq!(classify_account("system"), AccountType::System);
    }

    #[test]
    fn test_classify_eth_implicit_accounts() {
        // Valid ETH-implicit accounts
        assert_eq!(
            classify_account("0x1234567890abcdef1234567890abcdef12345678"),
            AccountType::EthImplicit
        );
        assert_eq!(
            classify_account("0xabcdef1234567890abcdef1234567890abcdef12"),
            AccountType::EthImplicit
        );

        // Invalid ETH-implicit accounts
        assert_ne!(
            classify_account("0x1234567890ABCDEF1234567890ABCDEF12345678"), // uppercase
            AccountType::EthImplicit
        );
        assert_ne!(
            classify_account("0x1234567890abcdef1234567890abcdef123456"), // too short
            AccountType::EthImplicit
        );
        assert_ne!(
            classify_account("0x1234567890abcdef1234567890abcdef123456789"), // too long
            AccountType::EthImplicit
        );
        assert_ne!(
            classify_account("1234567890abcdef1234567890abcdef12345678"), // no 0x prefix
            AccountType::EthImplicit
        );
    }

    #[test]
    fn test_classify_near_implicit_accounts() {
        // Known problematic account from integration test
        let problematic_account =
            "208ed1a73ac5a786c87794509a30a8a7f91fa8b639e14241b1612d9bd85956df";
        assert_eq!(
            classify_account(problematic_account),
            AccountType::Implicit,
            "This account should be classified as implicit"
        );

        // Other valid NEAR-implicit accounts
        assert_eq!(
            classify_account("7b893046785b433b47fcc6e0d303ad4c1b550c12f498e6df46da509dae9446c2"),
            AccountType::Implicit
        );
        assert_eq!(
            classify_account("3529f612fc44c9e8600ba0004437cb98fd3c2ec126c984e374ed783a40b58d30"),
            AccountType::Implicit
        );

        // Valid hex string - all lowercase
        assert_eq!(
            classify_account("abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"),
            AccountType::Implicit
        );

        // Invalid NEAR-implicit accounts
        assert_ne!(
            classify_account("ABCDEF1234567890ABCDEF1234567890ABCDEF1234567890ABCDEF1234567890"), // uppercase
            AccountType::Implicit
        );
        assert_ne!(
            classify_account("abcdef1234567890abcdef1234567890abcdef1234567890abcdef123456789"), // too short
            AccountType::Implicit
        );
        assert_ne!(
            classify_account("abcdef1234567890abcdef1234567890abcdef1234567890abcdef12345678901"), // too long
            AccountType::Implicit
        );
        assert_ne!(
            classify_account("abcdef1234567890abcdef1234567890abcdef1234567890abcdef123456789g"), // invalid char
            AccountType::Implicit
        );
    }

    #[test]
    fn test_classify_named_accounts() {
        assert_eq!(classify_account("alice.near"), AccountType::Named);
        assert_eq!(classify_account("example.testnet"), AccountType::Named);
        assert_eq!(classify_account("wrap.near"), AccountType::Named);
        assert_eq!(classify_account("v2.ref-finance.near"), AccountType::Named);
        assert_eq!(classify_account("token.sweat"), AccountType::Named);
        assert_eq!(classify_account("app_name-v1.near"), AccountType::Named);
        assert_eq!(classify_account("user123"), AccountType::Named);
    }

    #[test]
    fn test_classify_lockup_accounts() {
        assert_eq!(classify_account("lockup.near"), AccountType::Lockup);
        assert_eq!(classify_account("user.lockup.near"), AccountType::Lockup);
        assert_eq!(
            classify_account("lockup-contract.near"),
            AccountType::Lockup
        );
    }

    #[test]
    fn test_is_near_implicit_account_edge_cases() {
        // Test the exact problematic account
        assert!(is_near_implicit_account(
            "208ed1a73ac5a786c87794509a30a8a7f91fa8b639e14241b1612d9bd85956df"
        ));

        // Test various edge cases
        assert!(!is_near_implicit_account("")); // empty
        assert!(!is_near_implicit_account("abc")); // too short
        assert!(!is_near_implicit_account("abcdef")); // still too short

        // 63 characters (one short)
        assert!(!is_near_implicit_account(
            "abcdef1234567890abcdef1234567890abcdef1234567890abcdef123456789"
        ));

        // 65 characters (one too long)
        assert!(!is_near_implicit_account(
            "abcdef1234567890abcdef1234567890abcdef1234567890abcdef12345678901"
        ));

        // 64 characters with invalid character
        assert!(!is_near_implicit_account(
            "abcdef1234567890abcdef1234567890abcdef1234567890abcdef123456789g"
        ));

        // 64 characters with uppercase
        assert!(!is_near_implicit_account(
            "ABCDEF1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
        ));
    }

    #[test]
    fn test_is_eth_implicit_account_edge_cases() {
        // Valid cases
        assert!(is_eth_implicit_account(
            "0x1234567890abcdef1234567890abcdef12345678"
        ));
        assert!(is_eth_implicit_account(
            "0xabcdef1234567890abcdef1234567890abcdef12"
        ));

        // Invalid cases
        assert!(!is_eth_implicit_account(
            "0x1234567890ABCDEF1234567890abcdef12345678"
        )); // uppercase
        assert!(!is_eth_implicit_account(
            "0x1234567890abcdef1234567890abcdef123456"
        )); // too short
        assert!(!is_eth_implicit_account(
            "0x1234567890abcdef1234567890abcdef123456789"
        )); // too long
        assert!(!is_eth_implicit_account(
            "1234567890abcdef1234567890abcdef12345678"
        )); // no 0x
        assert!(!is_eth_implicit_account("0x")); // just prefix
        assert!(!is_eth_implicit_account("")); // empty
    }

    #[test]
    fn test_validate_account_id() {
        // Valid account IDs
        assert!(validate_account_id("alice.near").is_ok());
        assert!(validate_account_id("system").is_ok());
        assert!(validate_account_id("0x1234567890abcdef1234567890abcdef12345678").is_ok());
        assert!(validate_account_id(
            "208ed1a73ac5a786c87794509a30a8a7f91fa8b639e14241b1612d9bd85956df"
        )
        .is_ok());
        assert!(validate_account_id("app-name_v1.near").is_ok());

        // Invalid account IDs
        assert!(validate_account_id("").is_err()); // empty
        assert!(validate_account_id(&"a".repeat(65)).is_err()); // too long
        assert!(validate_account_id("alice.NEAR").is_err()); // uppercase in named account
        assert!(validate_account_id("alice@near").is_err()); // invalid character
        assert!(validate_account_id("alice near").is_err()); // space
    }

    #[test]
    fn test_get_parent_account() {
        assert_eq!(
            get_parent_account("sub.example.near"),
            Some("example.near".to_string())
        );
        assert_eq!(
            get_parent_account("app.sub.example.near"),
            Some("sub.example.near".to_string())
        );
        assert_eq!(get_parent_account("alice.near"), Some("near".to_string()));
        assert_eq!(get_parent_account("simple"), None);
        assert_eq!(
            get_parent_account("208ed1a73ac5a786c87794509a30a8a7f91fa8b639e14241b1612d9bd85956df"),
            None
        );
    }

    #[test]
    fn test_account_type_string_conversion() {
        assert_eq!(AccountType::System.as_str(), "system");
        assert_eq!(AccountType::Implicit.as_str(), "implicit");
        assert_eq!(AccountType::EthImplicit.as_str(), "eth_implicit");
        assert_eq!(AccountType::Named.as_str(), "named");
        assert_eq!(AccountType::Lockup.as_str(), "lockup");

        assert_eq!(String::from(AccountType::System), "system");
        assert_eq!(String::from(AccountType::Implicit), "implicit");
    }

    /// Test specifically for the bug found in integration tests
    #[test]
    fn test_problematic_accounts_from_integration() {
        // These accounts were misclassified as "named" instead of "implicit"
        let problematic_accounts = [
            "208ed1a73ac5a786c87794509a30a8a7f91fa8b639e14241b1612d9bd85956df",
            "3529f612fc44c9e8600ba0004437cb98fd3c2ec126c984e374ed783a40b58d30",
            "7b893046785b433b47fcc6e0d303ad4c1b550c12f498e6df46da509dae9446c2",
        ];

        for account_id in &problematic_accounts {
            // Verify they meet implicit account criteria
            assert_eq!(account_id.len(), 64, "Account should be 64 characters");
            assert!(
                account_id.chars().all(|c| c.is_ascii_hexdigit()),
                "Account should be all hex: {}",
                account_id
            );
            assert!(
                account_id.chars().all(|c| !c.is_ascii_uppercase()),
                "Account should be all lowercase: {}",
                account_id
            );

            // They should be classified as implicit
            assert_eq!(
                classify_account(account_id),
                AccountType::Implicit,
                "Account {} should be classified as implicit",
                account_id
            );

            // Helper functions should also work correctly
            assert!(
                is_near_implicit_account(account_id),
                "is_near_implicit_account should return true for {}",
                account_id
            );
        }
    }
}
