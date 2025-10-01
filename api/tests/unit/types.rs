use api::types::{DatabaseHealth, HealthResponse};
use chrono::{TimeZone, Utc};
use serde_json::json;

#[test]
fn health_response_serialization_skips_optional_fields() {
    let response = HealthResponse {
        status: "healthy".to_string(),
        timestamp: Utc
            .with_ymd_and_hms(2025, 1, 1, 0, 0, 0)
            .unwrap()
            .to_rfc3339(),
        version: "0.1.0".to_string(),
        database: DatabaseHealth {
            connected: true,
            response_time_ms: None,
            error: None,
        },
    };

    let value = serde_json::to_value(&response).expect("serialize");
    let db = value
        .get("database")
        .expect("database field present")
        .as_object()
        .expect("database object");

    assert!(!db.contains_key("response_time_ms"));
    assert!(!db.contains_key("error"));
}

#[test]
fn database_health_optional_fields_round_trip() {
    let payload = json!({
        "connected": false,
        "response_time_ms": 32,
        "error": "timeout",
    });

    let health: DatabaseHealth = serde_json::from_value(payload.clone()).expect("deserialize");
    assert!(!health.connected);
    assert_eq!(health.response_time_ms, Some(32));
    assert_eq!(health.error.as_deref(), Some("timeout"));

    let serialized = serde_json::to_value(&health).expect("serialize");
    assert_eq!(serialized, payload);
}
