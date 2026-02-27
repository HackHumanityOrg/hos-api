use anyhow::{anyhow, Result};
use serde_json;
use serde_json::Value as JsonValue;
use std::str::FromStr;
use tracing::debug;

use crate::modules::tokens::types::{EventLog, EventType, ParsedTokenEvent, TokenStandard};

const EVENT_JSON_PREFIX: &str = "EVENT_JSON:";

pub fn parse_event_log(log: &str) -> Result<ParsedTokenEvent> {
    // Check if log starts with EVENT_JSON prefix
    let json_str = log
        .strip_prefix(EVENT_JSON_PREFIX)
        .ok_or_else(|| anyhow!("Log does not start with EVENT_JSON prefix"))?;

    // Parse the JSON
    let event_log: EventLog =
        serde_json::from_str(json_str).map_err(|e| anyhow!("Failed to parse EVENT_JSON: {}", e))?;

    // Validate standard
    let standard = TokenStandard::from_str(&event_log.standard)
        .map_err(|e| anyhow!("Unsupported token standard: {}", e))?;

    // Validate event type
    let event_type = EventType::from_str(&event_log.event)
        .map_err(|e| anyhow!("Unsupported event type: {}", e))?;

    // Validate that the event type matches the standard
    validate_event_for_standard(&standard, &event_type)?;

    let normalized_data = match event_log.data {
        JsonValue::Array(items) => JsonValue::Array(items),
        JsonValue::Object(obj) => JsonValue::Array(vec![JsonValue::Object(obj)]),
        other => {
            return Err(anyhow!(
                "Invalid token event data type: expected array/object, got {}",
                other
            ))
        }
    };

    // Create the parsed event
    Ok(ParsedTokenEvent::new(
        standard,
        event_log.version,
        event_type,
        normalized_data,
    ))
}

fn validate_event_for_standard(standard: &TokenStandard, event_type: &EventType) -> Result<()> {
    let valid = match (standard, event_type) {
        // NEP-141 events
        (TokenStandard::Nep141, EventType::FtMint)
        | (TokenStandard::Nep141, EventType::FtBurn)
        | (TokenStandard::Nep141, EventType::FtTransfer) => true,

        // NEP-171 events
        (TokenStandard::Nep171, EventType::NftMint)
        | (TokenStandard::Nep171, EventType::NftBurn)
        | (TokenStandard::Nep171, EventType::NftTransfer) => true,

        // NEP-245 events
        (TokenStandard::Nep245, EventType::MtMint)
        | (TokenStandard::Nep245, EventType::MtBurn)
        | (TokenStandard::Nep245, EventType::MtTransfer) => true,

        _ => false,
    };

    if !valid {
        return Err(anyhow!(
            "Event type {} is not valid for standard {}",
            event_type.as_str(),
            standard.as_str()
        ));
    }

    Ok(())
}

pub fn is_token_event_log(log: &str) -> bool {
    if !log.starts_with(EVENT_JSON_PREFIX) {
        return false;
    }

    // Try to parse and check if it's a token event
    match parse_event_log(log) {
        Ok(_) => true,
        Err(e) => {
            debug!("Log is not a token event: {}", e);
            false
        }
    }
}

pub fn extract_token_events_from_logs(logs: &[String]) -> Vec<(usize, ParsedTokenEvent)> {
    let mut events = Vec::new();

    for (index, log) in logs.iter().enumerate() {
        if let Ok(event) = parse_event_log(log) {
            events.push((index, event));
        }
    }

    if !events.is_empty() {
        debug!(
            "Extracted {} token events from {} logs",
            events.len(),
            logs.len()
        );
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_nep141_transfer() {
        let log = r#"EVENT_JSON:{"standard":"nep141","version":"1.0.0","event":"ft_transfer","data":[{"old_owner_id":"alice.near","new_owner_id":"bob.near","amount":"1000000000000000000000000","memo":"test transfer"}]}"#;

        let event = parse_event_log(log).unwrap();
        assert_eq!(event.standard, TokenStandard::Nep141);
        assert_eq!(event.version, "1.0.0");
        assert_eq!(event.event_type, EventType::FtTransfer);
        assert_eq!(event.affected_accounts, vec!["alice.near", "bob.near"]);
        assert!(event.data.is_array());
    }

    #[test]
    fn test_parse_nep171_mint() {
        let log = r#"EVENT_JSON:{"standard":"nep171","version":"1.0.0","event":"nft_mint","data":[{"owner_id":"alice.near","token_ids":["1","2","3"]}]}"#;

        let event = parse_event_log(log).unwrap();
        assert_eq!(event.standard, TokenStandard::Nep171);
        assert_eq!(event.event_type, EventType::NftMint);
        assert_eq!(event.affected_accounts, vec!["alice.near"]);
    }

    #[test]
    fn test_parse_nep245_transfer() {
        let log = r#"EVENT_JSON:{"standard":"nep245","version":"1.0.0","event":"mt_transfer","data":[{"old_owner_id":"alice.near","new_owner_id":"bob.near","token_ids":["sword:1","gold"],"amounts":["1","1000"]}]}"#;

        let event = parse_event_log(log).unwrap();
        assert_eq!(event.standard, TokenStandard::Nep245);
        assert_eq!(event.event_type, EventType::MtTransfer);
        assert_eq!(event.affected_accounts, vec!["alice.near", "bob.near"]);
    }

    #[test]
    fn test_invalid_standard() {
        let log =
            r#"EVENT_JSON:{"standard":"erc20","version":"1.0.0","event":"transfer","data":[{}]}"#;

        let result = parse_event_log(log);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unsupported token standard"));
    }

    #[test]
    fn test_mismatched_event_and_standard() {
        let log = r#"EVENT_JSON:{"standard":"nep141","version":"1.0.0","event":"nft_transfer","data":[{}]}"#;

        let result = parse_event_log(log);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("is not valid for standard"));
    }

    #[test]
    fn test_not_event_json() {
        let log = "Regular log message";

        let result = parse_event_log(log);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("EVENT_JSON prefix"));
    }

    #[test]
    fn test_is_token_event_log() {
        let valid_log = r#"EVENT_JSON:{"standard":"nep141","version":"1.0.0","event":"ft_transfer","data":[{}]}"#;
        let invalid_log = "Regular log message";

        assert!(is_token_event_log(valid_log));
        assert!(!is_token_event_log(invalid_log));
    }

    #[test]
    fn test_object_data_is_normalized_to_array() {
        let log = r#"EVENT_JSON:{"standard":"nep141","version":"1.0.1","event":"ft_mint","data":{"owner_id":"alice.near","amount":"10"}}"#;

        let event = parse_event_log(log).unwrap();
        assert_eq!(event.version, "1.0.1");
        assert!(event.data.is_array());
        assert_eq!(event.affected_accounts, vec!["alice.near"]);
    }
}
