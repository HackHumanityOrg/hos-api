/// Account classification utilities for NEAR Protocol accounts.
use near_account_id::{AccountIdRef, AccountType as NearAccountType};
use tracing::trace;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountType {
    System,
    Named,
    Implicit,
    EthImplicit,
    Deterministic,
}

impl AccountType {
    pub fn as_str(&self) -> &'static str {
        match self {
            AccountType::System => "system",
            AccountType::Named => "named",
            AccountType::Implicit => "implicit",
            AccountType::EthImplicit => "eth_implicit",
            AccountType::Deterministic => "deterministic",
        }
    }
}

impl From<AccountType> for String {
    fn from(account_type: AccountType) -> Self {
        account_type.as_str().to_string()
    }
}

/// Classify account type using canonical near-account-id rules.
pub fn classify_account(account_id: &str) -> AccountType {
    let account_type = if account_id == "system" {
        AccountType::System
    } else if let Ok(id) = AccountIdRef::new(account_id) {
        match id.get_account_type() {
            NearAccountType::NamedAccount => AccountType::Named,
            NearAccountType::NearImplicitAccount => AccountType::Implicit,
            NearAccountType::EthImplicitAccount => AccountType::EthImplicit,
            NearAccountType::NearDeterministicAccount => AccountType::Deterministic,
        }
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

pub fn is_eth_implicit_account(account_id: &str) -> bool {
    matches!(
        AccountIdRef::new(account_id),
        Ok(id) if id.get_account_type() == NearAccountType::EthImplicitAccount
    )
}

pub fn is_near_implicit_account(account_id: &str) -> bool {
    matches!(
        AccountIdRef::new(account_id),
        Ok(id) if id.get_account_type() == NearAccountType::NearImplicitAccount
    )
}

pub fn is_near_deterministic_account(account_id: &str) -> bool {
    matches!(
        AccountIdRef::new(account_id),
        Ok(id) if id.get_account_type() == NearAccountType::NearDeterministicAccount
    )
}

/// Validate account ID format according to canonical NEAR account rules.
pub fn validate_account_id(account_id: &str) -> Result<(), String> {
    AccountIdRef::new(account_id)
        .map(|_| ())
        .map_err(|err| format!("Invalid NEAR account ID: {err}"))
}

/// Extract parent account ID for sub-accounts.
pub fn get_parent_account(account_id: &str) -> Option<String> {
    let id = AccountIdRef::new(account_id).ok()?;
    id.get_parent_account_id().map(|parent| parent.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_system_account() {
        assert_eq!(classify_account("system"), AccountType::System);
    }

    #[test]
    fn classify_account_types() {
        assert_eq!(classify_account("alice.near"), AccountType::Named);
        assert_eq!(
            classify_account("0x1234567890abcdef1234567890abcdef12345678"),
            AccountType::EthImplicit
        );
        assert_eq!(
            classify_account("208ed1a73ac5a786c87794509a30a8a7f91fa8b639e14241b1612d9bd85956df"),
            AccountType::Implicit
        );
        assert_eq!(
            classify_account("0s1234567890abcdef1234567890abcdef12345678"),
            AccountType::Deterministic
        );
    }

    #[test]
    fn validate_account_id_uses_canonical_rules() {
        assert!(validate_account_id("alice.near").is_ok());
        assert!(validate_account_id("a").is_err());
        assert!(validate_account_id("a..near").is_err());
        assert!(validate_account_id("-alice.near").is_err());
        assert!(validate_account_id("alice.near-").is_err());
    }

    #[test]
    fn helper_predicates_follow_canonical_types() {
        assert!(is_eth_implicit_account(
            "0x1234567890abcdef1234567890abcdef12345678"
        ));
        assert!(!is_eth_implicit_account(
            "0x1234567890ABCDEF1234567890abcdef12345678"
        ));

        assert!(is_near_implicit_account(
            "208ed1a73ac5a786c87794509a30a8a7f91fa8b639e14241b1612d9bd85956df"
        ));
        assert!(!is_near_implicit_account(
            "208ed1a73ac5a786c87794509a30a8a7f91fa8b639e14241b1612d9bd85956d"
        ));

        assert!(is_near_deterministic_account(
            "0s1234567890abcdef1234567890abcdef12345678"
        ));
        assert!(!is_near_deterministic_account("0s.alice.near"));
    }

    #[test]
    fn get_parent_account_for_subaccounts() {
        assert_eq!(
            get_parent_account("app.alice.near"),
            Some("alice.near".to_string())
        );
        assert_eq!(get_parent_account("alice.near"), Some("near".to_string()));
        assert_eq!(
            get_parent_account("208ed1a73ac5a786c87794509a30a8a7f91fa8b639e14241b1612d9bd85956df"),
            None
        );
    }

    #[test]
    fn account_type_to_string() {
        assert_eq!(AccountType::System.as_str(), "system");
        assert_eq!(AccountType::Named.as_str(), "named");
        assert_eq!(AccountType::Implicit.as_str(), "implicit");
        assert_eq!(AccountType::EthImplicit.as_str(), "eth_implicit");
        assert_eq!(AccountType::Deterministic.as_str(), "deterministic");
    }
}
