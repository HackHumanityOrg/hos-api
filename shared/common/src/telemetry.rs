//! Centralized logging and tracing setup.
//!
//! This module initializes `tracing` subscribers for all services and emits
//! structured JSON logs.

use crate::config::TelemetryConfig as ConfigTelemetry;
use anyhow::{Context, Result};
use serde_json::{Map as JsonMap, Value as JsonValue};
use tracing::info;
use tracing_subscriber::{
    layer::{Layer, SubscriberExt},
    util::SubscriberInitExt,
    EnvFilter, Registry,
};

/// Runtime telemetry setup for a service process.
pub struct TelemetrySetup {
    /// Service name (for example: "api", "discourse-indexer").
    pub service_name: String,
    /// Configuration from service config.toml.
    pub config: ConfigTelemetry,
    /// Log level filter.
    pub log_level: String,
}

impl TelemetrySetup {
    /// Create telemetry setup from config values.
    pub fn from_config(
        service_name: &str,
        config: &ConfigTelemetry,
        log_level: &str,
    ) -> Result<Self> {
        Ok(Self {
            service_name: service_name.to_string(),
            config: config.clone(),
            log_level: log_level.to_string(),
        })
    }
}

/// Guard type kept by applications for API compatibility.
pub struct TelemetryGuard;

/// Initialize process-wide tracing with JSON logs.
pub fn init_telemetry(setup: TelemetrySetup) -> Result<TelemetryGuard> {
    let env_filter = build_env_filter(&setup.service_name, &setup.log_level)?;
    let fmt_layer = build_fmt_layer();
    let subscriber = Registry::default().with(fmt_layer).with(env_filter);

    subscriber
        .try_init()
        .context("Failed to initialize tracing subscriber")?;

    info!(
        service = %setup.service_name,
        log_level = %setup.log_level,
        telemetry_enabled = setup.config.enabled,
        "Initialized JSON logging and tracing"
    );

    Ok(TelemetryGuard)
}

/// Initialize basic tracing with JSON logs.
pub fn init_basic_tracing(log_level: &str) -> Result<()> {
    let env_filter = build_env_filter("common", log_level)?;
    let fmt_layer = build_fmt_layer();
    let subscriber = Registry::default().with(fmt_layer).with(env_filter);

    subscriber
        .try_init()
        .context("Failed to initialize tracing subscriber")?;
    Ok(())
}

fn build_env_filter(service_name: &str, log_level: &str) -> Result<EnvFilter> {
    let base_log_filter = std::env::var("RUST_LOG").unwrap_or_else(|_| log_level.to_string());

    let env_filter = EnvFilter::try_new(base_log_filter)
        .context("Invalid log level filter")?
        .add_directive("hyper_util::client::legacy::pool=info".parse()?)
        .add_directive("reqwest::connect=info".parse()?)
        .add_directive("tokio_util=info".parse()?)
        .add_directive("tower=info".parse()?)
        .add_directive("h2=info".parse()?)
        .add_directive(format!("{}={}", service_name.replace('-', "_"), log_level).parse()?)
        .add_directive(format!("api={}", log_level).parse()?)
        .add_directive(format!("common={}", log_level).parse()?)
        .add_directive(format!("db_core={}", log_level).parse()?);

    Ok(env_filter)
}

fn build_fmt_layer() -> Box<dyn Layer<Registry> + Send + Sync + 'static> {
    Box::new(
        tracing_subscriber::fmt::layer()
            .with_target(true)
            .with_thread_ids(true)
            .with_thread_names(true)
            .json()
            .flatten_event(true),
    )
}

/// Lightweight telemetry attribute used by compatibility metric helpers.
#[derive(Debug, Clone)]
pub struct KeyValue {
    key: String,
    value: JsonValue,
}

impl KeyValue {
    pub fn new<K, V>(key: K, value: V) -> Self
    where
        K: Into<String>,
        V: Into<MetricValue>,
    {
        Self {
            key: key.into(),
            value: value.into().into_json(),
        }
    }
}

/// Value type for [`KeyValue`] helper construction.
#[derive(Debug, Clone)]
pub enum MetricValue {
    Bool(bool),
    I64(i64),
    U64(u64),
    F64(f64),
    String(String),
    Json(JsonValue),
}

impl MetricValue {
    fn into_json(self) -> JsonValue {
        match self {
            Self::Bool(value) => JsonValue::from(value),
            Self::I64(value) => JsonValue::from(value),
            Self::U64(value) => JsonValue::from(value),
            Self::F64(value) => JsonValue::from(value),
            Self::String(value) => JsonValue::from(value),
            Self::Json(value) => value,
        }
    }
}

impl From<bool> for MetricValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<i64> for MetricValue {
    fn from(value: i64) -> Self {
        Self::I64(value)
    }
}

impl From<u64> for MetricValue {
    fn from(value: u64) -> Self {
        Self::U64(value)
    }
}

impl From<usize> for MetricValue {
    fn from(value: usize) -> Self {
        Self::U64(value as u64)
    }
}

impl From<f64> for MetricValue {
    fn from(value: f64) -> Self {
        Self::F64(value)
    }
}

impl From<String> for MetricValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for MetricValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<JsonValue> for MetricValue {
    fn from(value: JsonValue) -> Self {
        Self::Json(value)
    }
}

fn attributes_to_json(attributes: &[KeyValue]) -> JsonValue {
    let mut map = JsonMap::new();
    for attribute in attributes {
        map.insert(attribute.key.clone(), attribute.value.clone());
    }
    JsonValue::Object(map)
}

fn emit_metric(kind: &str, name: &'static str, value: f64, attributes: &[KeyValue]) {
    let attrs = attributes_to_json(attributes);
    tracing::info!(
        target: "metrics",
        metric_kind = kind,
        metric_name = name,
        metric_value = value,
        metric_attributes = %attrs,
        "metric"
    );
}

/// Record a custom metric value.
pub fn record_metric(name: &'static str, value: f64, attributes: &[KeyValue]) {
    emit_metric("gauge", name, value, attributes);
}

/// Record a counter increment.
pub fn increment_counter(name: &'static str, attributes: &[KeyValue]) {
    emit_metric("counter", name, 1.0, attributes);
}

/// Histogram handle used for compatibility with existing call sites.
pub struct DurationHistogram {
    name: &'static str,
}

impl DurationHistogram {
    pub fn record(&self, value: f64, attributes: &[KeyValue]) {
        emit_metric("histogram", self.name, value, attributes);
    }
}

/// Create a histogram for recording durations.
pub fn duration_histogram(name: &'static str) -> DurationHistogram {
    DurationHistogram { name }
}
