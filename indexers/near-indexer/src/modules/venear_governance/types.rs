use anyhow::{anyhow, Result};
use serde_json::Value as JsonValue;
use sha2::{Digest, Sha256};

pub const EVENT_JSON_PREFIX: &str = "EVENT_JSON:";

#[derive(Debug, Clone)]
pub struct ParsedEventLog {
    pub standard: Option<String>,
    pub event_type: Option<String>,
    pub data_items: Vec<JsonValue>,
    pub parse_error: Option<String>,
}

pub fn parse_event_log(log: &str) -> ParsedEventLog {
    if !log.starts_with(EVENT_JSON_PREFIX) {
        return ParsedEventLog {
            standard: None,
            event_type: None,
            data_items: Vec::new(),
            parse_error: Some("log does not start with EVENT_JSON prefix".to_string()),
        };
    }

    let json_part = &log[EVENT_JSON_PREFIX.len()..];
    let parsed: JsonValue = match serde_json::from_str(json_part) {
        Ok(value) => value,
        Err(err) => {
            return ParsedEventLog {
                standard: None,
                event_type: None,
                data_items: Vec::new(),
                parse_error: Some(format!("failed to parse EVENT_JSON payload: {err}")),
            };
        }
    };

    let standard = parsed
        .get("standard")
        .and_then(JsonValue::as_str)
        .map(ToString::to_string);
    let event_type = parsed
        .get("event")
        .and_then(JsonValue::as_str)
        .map(ToString::to_string);

    let Some(data_value) = parsed.get("data") else {
        return ParsedEventLog {
            standard,
            event_type,
            data_items: Vec::new(),
            parse_error: Some("EVENT_JSON payload missing data field".to_string()),
        };
    };

    let data_items = match data_value {
        JsonValue::Array(items) => items.clone(),
        JsonValue::Object(_) => vec![data_value.clone()],
        _ => {
            return ParsedEventLog {
                standard,
                event_type,
                data_items: Vec::new(),
                parse_error: Some("EVENT_JSON data must be array or object".to_string()),
            };
        }
    };

    ParsedEventLog {
        standard,
        event_type,
        data_items,
        parse_error: None,
    }
}

pub fn is_outcome_success(status: &JsonValue) -> bool {
    let JsonValue::Object(map) = status else {
        return false;
    };

    map.contains_key("SuccessValue") || map.contains_key("SuccessReceiptId")
}

pub fn derive_lockup_account_id(owner_account_id: &str, lockup_suffix: &str) -> String {
    let digest = Sha256::digest(owner_account_id.as_bytes());
    let prefix = hex::encode(&digest[..20]);
    format!("{prefix}{lockup_suffix}")
}

pub fn json_string(value: &JsonValue, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(JsonValue::as_str)
        .map(ToString::to_string)
}

pub fn json_u64(value: &JsonValue, key: &str) -> Option<u64> {
    let field = value.get(key)?;
    match field {
        JsonValue::Number(number) => number.as_u64(),
        JsonValue::String(text) => text.parse().ok(),
        _ => None,
    }
}

pub fn json_u128(value: &JsonValue, key: &str) -> Option<u128> {
    let field = value.get(key)?;
    match field {
        JsonValue::Number(number) => number.as_u64().map(|v| v as u128),
        JsonValue::String(text) => text.parse().ok(),
        _ => None,
    }
}

pub fn parse_args_json(args: &[u8]) -> Option<JsonValue> {
    if args.is_empty() {
        return Some(JsonValue::Object(serde_json::Map::new()));
    }

    serde_json::from_slice(args).ok().or_else(|| {
        let text = std::str::from_utf8(args).ok()?;
        serde_json::from_str(text).ok()
    })
}

pub fn parse_i32_field(value: &JsonValue, key: &str) -> Result<i32> {
    let raw = value
        .get(key)
        .ok_or_else(|| anyhow!("missing {key}"))?
        .clone();

    let parsed = match raw {
        JsonValue::Number(n) => n
            .as_i64()
            .ok_or_else(|| anyhow!("invalid {key}: expected integer"))?,
        JsonValue::String(s) => s
            .parse::<i64>()
            .map_err(|err| anyhow!("invalid {key}: {err}"))?,
        _ => return Err(anyhow!("invalid {key}: unsupported type")),
    };

    i32::try_from(parsed).map_err(|_| anyhow!("invalid {key}: out of i32 range"))
}

pub fn apply_balance_delta(previous_balance: u128, event_type: &str, amount: u128) -> u128 {
    match event_type {
        "ft_mint" => previous_balance.saturating_add(amount),
        "ft_burn" => previous_balance.saturating_sub(amount),
        _ => previous_balance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_event_log_parses_array_data() {
        let log = r#"EVENT_JSON:{"standard":"venear","version":"1.0.0","event":"add_vote","data":[{"account_id":"alice.near","proposal_id":1,"vote":2,"account_balance":"10"}]}"#;
        let parsed = parse_event_log(log);
        assert_eq!(parsed.standard.as_deref(), Some("venear"));
        assert_eq!(parsed.event_type.as_deref(), Some("add_vote"));
        assert!(parsed.parse_error.is_none());
        assert_eq!(parsed.data_items.len(), 1);
    }

    #[test]
    fn parse_event_log_normalizes_object_data() {
        let log = r#"EVENT_JSON:{"standard":"nep141","version":"1.0.0","event":"ft_mint","data":{"owner_id":"alice.near","amount":"5"}}"#;
        let parsed = parse_event_log(log);
        assert!(parsed.parse_error.is_none());
        assert_eq!(parsed.data_items.len(), 1);
    }

    #[test]
    fn parse_event_log_returns_error_for_invalid_json() {
        let log = "EVENT_JSON:not-json";
        let parsed = parse_event_log(log);
        assert!(parsed.parse_error.is_some());
    }

    #[test]
    fn parse_event_log_returns_error_for_missing_prefix() {
        let parsed = parse_event_log("{\"standard\":\"venear\"}");
        assert_eq!(
            parsed.parse_error.as_deref(),
            Some("log does not start with EVENT_JSON prefix")
        );
        assert!(parsed.standard.is_none());
        assert!(parsed.event_type.is_none());
        assert!(parsed.data_items.is_empty());
    }

    #[test]
    fn parse_event_log_returns_error_for_missing_data() {
        let log = r#"EVENT_JSON:{"standard":"venear","event":"add_vote"}"#;
        let parsed = parse_event_log(log);

        assert_eq!(parsed.standard.as_deref(), Some("venear"));
        assert_eq!(parsed.event_type.as_deref(), Some("add_vote"));
        assert_eq!(
            parsed.parse_error.as_deref(),
            Some("EVENT_JSON payload missing data field")
        );
        assert!(parsed.data_items.is_empty());
    }

    #[test]
    fn parse_event_log_returns_error_for_invalid_data_shape() {
        let log = r#"EVENT_JSON:{"standard":"venear","event":"add_vote","data":"nope"}"#;
        let parsed = parse_event_log(log);

        assert_eq!(parsed.standard.as_deref(), Some("venear"));
        assert_eq!(parsed.event_type.as_deref(), Some("add_vote"));
        assert_eq!(
            parsed.parse_error.as_deref(),
            Some("EVENT_JSON data must be array or object")
        );
        assert!(parsed.data_items.is_empty());
    }

    #[test]
    fn outcome_success_detection_works() {
        assert!(is_outcome_success(&json!({"SuccessValue": ""})));
        assert!(is_outcome_success(&json!({"SuccessReceiptId": "abc"})));
        assert!(!is_outcome_success(
            &json!({"Failure": {"ActionError": {}}})
        ));
        assert!(!is_outcome_success(&JsonValue::Null));
    }

    #[test]
    fn lockup_id_derivation_uses_expected_shape() {
        let derived = derive_lockup_account_id("alice.near", ".venear.dao");
        assert!(derived.ends_with(".venear.dao"));
        assert_eq!(derived.len(), 40 + ".venear.dao".len());
    }

    #[test]
    fn lockup_id_derivation_matches_known_vectors() {
        assert_eq!(
            derive_lockup_account_id("alice.near", ".venear.dao"),
            "2dd5dda540767b3a1aa33544bcba38042f4df6de.venear.dao"
        );
        assert_eq!(
            derive_lockup_account_id("bob.near", ".venear.dao"),
            "24341428553285e10e74a5f26f4638ac53afb28c.venear.dao"
        );
    }

    #[test]
    fn json_u64_handles_strings_and_invalid_values() {
        let payload = json!({
            "numeric": 42,
            "stringy": "43",
            "negative": -1,
            "not_numeric": "abc",
            "too_big": "18446744073709551616"
        });

        assert_eq!(json_u64(&payload, "numeric"), Some(42));
        assert_eq!(json_u64(&payload, "stringy"), Some(43));
        assert_eq!(json_u64(&payload, "negative"), None);
        assert_eq!(json_u64(&payload, "not_numeric"), None);
        assert_eq!(json_u64(&payload, "too_big"), None);
        assert_eq!(json_u64(&payload, "missing"), None);
    }

    #[test]
    fn json_u128_handles_large_string_values() {
        let payload = json!({
            "numeric": 99,
            "stringy": "340282366920938463463374607431768211455",
            "too_big": "340282366920938463463374607431768211456",
            "negative": -5
        });

        assert_eq!(json_u128(&payload, "numeric"), Some(99));
        assert_eq!(json_u128(&payload, "stringy"), Some(u128::MAX));
        assert_eq!(json_u128(&payload, "too_big"), None);
        assert_eq!(json_u128(&payload, "negative"), None);
        assert_eq!(json_u128(&payload, "missing"), None);
    }

    #[test]
    fn parse_args_json_handles_empty_and_invalid_utf8() {
        assert_eq!(parse_args_json(&[]), Some(json!({})));
        assert_eq!(
            parse_args_json(br#"{"owner_account_id":"alice.near"}"#),
            Some(json!({"owner_account_id":"alice.near"}))
        );

        let invalid_utf8 = vec![0xff, 0xfe, 0xfd];
        assert_eq!(parse_args_json(&invalid_utf8), None);
    }

    #[test]
    fn parse_i32_field_handles_bounds_and_errors() {
        let bounds = json!({
            "min": i32::MIN,
            "max": i32::MAX,
            "string": "10"
        });

        assert_eq!(parse_i32_field(&bounds, "min").unwrap(), i32::MIN);
        assert_eq!(parse_i32_field(&bounds, "max").unwrap(), i32::MAX);
        assert_eq!(parse_i32_field(&bounds, "string").unwrap(), 10);

        let out_of_range = json!({"vote": 2147483648_i64});
        let err = parse_i32_field(&out_of_range, "vote").unwrap_err();
        assert!(err.to_string().contains("out of i32 range"));

        let invalid_string = json!({"vote": "not-a-number"});
        let err = parse_i32_field(&invalid_string, "vote").unwrap_err();
        assert!(err.to_string().contains("invalid vote"));

        let invalid_type = json!({"vote": true});
        let err = parse_i32_field(&invalid_type, "vote").unwrap_err();
        assert!(err.to_string().contains("unsupported type"));

        let missing = json!({});
        let err = parse_i32_field(&missing, "vote").unwrap_err();
        assert!(err.to_string().contains("missing vote"));
    }

    #[test]
    fn apply_balance_delta_uses_cumulative_totals() {
        let first = apply_balance_delta(20, "ft_mint", 5);
        assert_eq!(first, 25);

        let second = apply_balance_delta(first, "ft_burn", 3);
        assert_eq!(second, 22);
    }

    #[test]
    fn apply_balance_delta_saturates_burn_underflow() {
        assert_eq!(apply_balance_delta(3, "ft_burn", 5), 0);
    }
}
