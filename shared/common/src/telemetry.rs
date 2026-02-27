//! Centralized telemetry module for Sentry integration.
//!
//! This module initializes tracing for all services and, when configured,
//! forwards errors, spans, breadcrumbs, and logs to Sentry.

use crate::config::TelemetryConfig as ConfigTelemetry;
use anyhow::{Context, Result};
use sentry::integrations::tracing::{self as sentry_tracing, EventFilter};
use sentry::ClientInitGuard;
use serde_json::{Map as JsonMap, Value as JsonValue};
use std::{collections::BTreeMap, time::Duration};
use tracing::info;
use tracing_subscriber::{
    layer::{Layer, SubscriberExt},
    util::SubscriberInitExt,
    EnvFilter, Registry,
};

/// Telemetry setup combining config.toml settings with environment values.
pub struct TelemetrySetup {
    /// Service name (for example: "api", "discourse-indexer").
    pub service_name: String,
    /// Configuration from service config.toml.
    pub config: ConfigTelemetry,
    /// Sentry DSN from environment.
    pub dsn: Option<String>,
    /// Log level filter.
    pub log_level: String,
    /// Preferred log formatting ("json" for structured logs, otherwise pretty).
    pub log_format: Option<String>,
}

impl TelemetrySetup {
    /// Create telemetry setup from config and environment.
    pub fn from_config(
        service_name: &str,
        config: &ConfigTelemetry,
        log_level: &str,
        log_format: Option<&str>,
    ) -> Result<Self> {
        let dsn = if config.enabled {
            match std::env::var("SENTRY_DSN").ok() {
                Some(dsn) => Some(dsn),
                None => {
                    tracing::warn!(
                        "SENTRY_DSN is not set; Sentry telemetry disabled for this service"
                    );
                    None
                }
            }
        } else {
            None
        };

        Ok(Self {
            service_name: service_name.to_string(),
            config: config.clone(),
            dsn,
            log_level: log_level.to_string(),
            log_format: log_format.map(str::to_string),
        })
    }

    /// Check if Sentry telemetry is available.
    pub fn is_sentry_available(&self) -> bool {
        self.config.enabled && self.dsn.is_some()
    }
}

fn env_var_non_empty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_deployment_environment(config_environment: &str) -> String {
    env_var_non_empty("APP_TELEMETRY__ENVIRONMENT")
        .or_else(|| env_var_non_empty("RAILWAY_ENVIRONMENT_NAME"))
        .or_else(|| env_var_non_empty("RAILWAY_ENVIRONMENT"))
        .unwrap_or_else(|| config_environment.to_string())
}

fn resolve_deployment_region(config_region: Option<&str>) -> String {
    env_var_non_empty("APP_TELEMETRY__DEPLOYMENT_REGION")
        .or_else(|| env_var_non_empty("DEPLOYMENT_REGION"))
        .or_else(|| env_var_non_empty("RAILWAY_REGION"))
        .or_else(|| env_var_non_empty("RAILWAY_DEPLOYMENT_REGION"))
        .or_else(|| env_var_non_empty("RAILWAY_REPLICA_REGION"))
        .or_else(|| config_region.map(|value| value.to_string()))
        .unwrap_or_else(|| "unknown".to_string())
}

fn normalize_release(
    service_name: &str,
    configured_release: Option<&str>,
    service_version: &str,
) -> String {
    match configured_release
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(release) if release.contains('@') => release.to_string(),
        Some(release) => format!("{service_name}@{release}"),
        None => format!("{service_name}@{service_version}"),
    }
}

fn resolve_release(service_name: &str, service_version: &str) -> String {
    normalize_release(
        service_name,
        env_var_non_empty("SENTRY_RELEASE").as_deref(),
        service_version,
    )
}

fn build_scope_tags(
    setup: &TelemetrySetup,
    deployment_environment: &str,
    deployment_region: &str,
) -> BTreeMap<String, String> {
    let mut tags: BTreeMap<String, String> = setup
        .config
        .tags
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();

    tags.insert("service.name".to_string(), setup.service_name.clone());
    tags.insert(
        "service.version".to_string(),
        setup.config.service_version.clone(),
    );
    tags.entry("service.component".to_string())
        .or_insert_with(|| setup.service_name.clone());
    tags.insert(
        "deployment.environment".to_string(),
        deployment_environment.to_string(),
    );
    tags.insert(
        "deployment.region".to_string(),
        deployment_region.to_string(),
    );

    tags
}

/// Telemetry guard that ensures Sentry flushes data on shutdown.
pub struct TelemetryGuard {
    sentry_guard: Option<ClientInitGuard>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        if self.sentry_guard.is_some() {
            info!("Flushing telemetry to Sentry before shutdown...");
        }
        self.sentry_guard.take();
    }
}

/// Initialize complete telemetry stack.
pub fn init_telemetry(setup: TelemetrySetup) -> Result<TelemetryGuard> {
    if !setup.is_sentry_available() {
        info!("Initializing basic tracing (Sentry not configured)");
        init_basic_tracing(&setup.log_level, setup.log_format.as_deref())?;
        return Ok(TelemetryGuard { sentry_guard: None });
    }

    let dsn = setup.dsn.clone().expect("checked by is_sentry_available");
    let release = resolve_release(&setup.service_name, &setup.config.service_version);

    let traces_sample_rate = if setup.config.enable_traces {
        setup.config.traces_sample_rate
    } else {
        0.0
    };

    let deployment_environment = resolve_deployment_environment(&setup.config.environment);
    let deployment_region = resolve_deployment_region(
        setup
            .config
            .tags
            .get("deployment.region")
            .map(String::as_str),
    );
    let scope_tags = build_scope_tags(&setup, &deployment_environment, &deployment_region);
    let log_scope_attributes: Vec<(String, sentry::protocol::LogAttribute)> = scope_tags
        .iter()
        .map(|(key, value)| (key.clone(), value.clone().into()))
        .collect();

    info!(
        "Initializing Sentry telemetry for service={} release={} environment={} region={}",
        setup.service_name, release, deployment_environment, deployment_region
    );

    let sentry_guard = sentry::init((
        dsn,
        sentry::ClientOptions {
            release: Some(release.into()),
            environment: Some(deployment_environment.clone().into()),
            traces_sample_rate,
            sample_rate: setup.config.event_sample_rate,
            enable_logs: setup.config.enable_logs,
            before_send_log: Some(std::sync::Arc::new(move |mut log| {
                for (key, value) in &log_scope_attributes {
                    log.attributes
                        .entry(key.clone())
                        .or_insert_with(|| value.clone());
                }
                Some(log)
            })),
            send_default_pii: setup.config.send_default_pii,
            shutdown_timeout: Duration::from_secs(setup.config.shutdown_timeout_sec),
            ..Default::default()
        },
    ));

    sentry::configure_scope(|scope| {
        for (key, value) in &scope_tags {
            scope.set_tag(key, value.clone());
        }
    });

    let env_filter = build_env_filter(&setup.service_name, &setup.log_level)?;
    let fmt_layer = build_fmt_layer(setup.log_format.as_deref());

    let enable_logs = setup.config.enable_logs;
    let enable_traces = setup.config.enable_traces;

    let sentry_layer = sentry_tracing::layer()
        .event_filter(move |metadata| match *metadata.level() {
            tracing::Level::TRACE => EventFilter::Ignore,
            tracing::Level::DEBUG => EventFilter::Breadcrumb,
            tracing::Level::INFO => {
                if enable_logs {
                    EventFilter::Breadcrumb | EventFilter::Log
                } else {
                    EventFilter::Breadcrumb
                }
            }
            tracing::Level::WARN => {
                if enable_logs {
                    EventFilter::Breadcrumb | EventFilter::Log
                } else {
                    EventFilter::Breadcrumb
                }
            }
            tracing::Level::ERROR => {
                if enable_logs {
                    EventFilter::Event | EventFilter::Log
                } else {
                    EventFilter::Event
                }
            }
        })
        .span_filter(move |metadata| {
            enable_traces
                && !matches!(
                    *metadata.level(),
                    tracing::Level::TRACE | tracing::Level::DEBUG
                )
        });

    let subscriber = Registry::default()
        .with(fmt_layer)
        .with(env_filter)
        .with(sentry_layer);

    subscriber
        .try_init()
        .context("Failed to initialize tracing subscriber")?;

    info!("Sentry telemetry initialization complete");

    Ok(TelemetryGuard {
        sentry_guard: Some(sentry_guard),
    })
}

/// Initialize basic tracing without Sentry.
pub fn init_basic_tracing(log_level: &str, log_format: Option<&str>) -> Result<()> {
    let env_filter = build_env_filter("common", log_level)?;
    let fmt_layer = build_fmt_layer(log_format);
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
        .add_directive("sentry=info".parse()?)
        .add_directive(format!("{}={}", service_name.replace('-', "_"), log_level).parse()?)
        .add_directive(format!("api={}", log_level).parse()?)
        .add_directive(format!("common={}", log_level).parse()?)
        .add_directive(format!("db_core={}", log_level).parse()?);

    Ok(env_filter)
}

fn build_fmt_layer(log_format: Option<&str>) -> Box<dyn Layer<Registry> + Send + Sync + 'static> {
    match log_format.map(|s| s.to_ascii_lowercase()) {
        Some(ref fmt) if fmt == "json" => Box::new(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(true)
                .with_thread_names(true)
                .json(),
        ),
        _ => Box::new(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_thread_names(false),
        ),
    }
}

/// Returns true when a Sentry client is active for the current process.
pub fn is_sentry_enabled() -> bool {
    sentry::Hub::current()
        .client()
        .map(|client| client.is_enabled())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn telemetry_setup_with_tags(tags: &[(&str, &str)]) -> TelemetrySetup {
        let mut config = ConfigTelemetry {
            service_version: "9.9.9".to_string(),
            ..Default::default()
        };

        config.tags = tags
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect::<HashMap<_, _>>();

        TelemetrySetup {
            service_name: "api".to_string(),
            config,
            dsn: None,
            log_level: "info".to_string(),
            log_format: Some("json".to_string()),
        }
    }

    #[test]
    fn normalize_release_prefixes_plain_release_with_service_name() {
        let release = normalize_release("api", Some("2026.02.27+sha.abcdef"), "0.1.0");
        assert_eq!(release, "api@2026.02.27+sha.abcdef");
    }

    #[test]
    fn normalize_release_preserves_scoped_release() {
        let release = normalize_release("api", Some("near-indexer@2026.02.27"), "0.1.0");
        assert_eq!(release, "near-indexer@2026.02.27");
    }

    #[test]
    fn build_scope_tags_sets_distinct_service_metadata() {
        let setup = telemetry_setup_with_tags(&[
            ("service.name", "incorrect-name"),
            ("service.component", "public-api"),
            ("service.namespace", "data-commons"),
            ("custom.tag", "custom-value"),
        ]);

        let tags = build_scope_tags(&setup, "production", "us-west-2");

        assert_eq!(tags.get("service.name").map(String::as_str), Some("api"));
        assert_eq!(
            tags.get("service.version").map(String::as_str),
            Some("9.9.9")
        );
        assert_eq!(
            tags.get("service.component").map(String::as_str),
            Some("public-api")
        );
        assert_eq!(
            tags.get("service.namespace").map(String::as_str),
            Some("data-commons")
        );
        assert_eq!(
            tags.get("deployment.environment").map(String::as_str),
            Some("production")
        );
        assert_eq!(
            tags.get("deployment.region").map(String::as_str),
            Some("us-west-2")
        );
        assert_eq!(
            tags.get("custom.tag").map(String::as_str),
            Some("custom-value")
        );
    }

    #[test]
    fn build_scope_tags_defaults_service_component_to_service_name() {
        let setup = telemetry_setup_with_tags(&[]);
        let tags = build_scope_tags(&setup, "staging", "eu-central-1");

        assert_eq!(
            tags.get("service.component").map(String::as_str),
            Some("api")
        );
    }
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
