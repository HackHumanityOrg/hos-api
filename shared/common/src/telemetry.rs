//! Centralized telemetry module for OpenTelemetry integration with Dash0
//!
//! This module provides a unified telemetry setup for logs, traces, and metrics
//! that can be used across all services in the monorepo.

use crate::config::TelemetryConfig as ConfigTelemetry;
use anyhow::{Context, Result};
use opentelemetry::{global, trace::TracerProvider, KeyValue};
use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_otlp::{LogExporter, MetricExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    logs::{BatchLogProcessor, LoggerProvider},
    metrics::{MeterProviderBuilder, PeriodicReader, SdkMeterProvider},
    propagation::TraceContextPropagator,
    trace::{self, BatchSpanProcessor},
    Resource,
};
use opentelemetry_semantic_conventions::resource::{SERVICE_NAME, SERVICE_VERSION};
use std::time::Duration;
use tracing::info;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

/// Telemetry setup combining config.toml settings with environment secrets
pub struct TelemetrySetup {
    /// Service name (e.g., "api", "indexer-1")
    pub service_name: String,
    /// Configuration from config.toml
    pub config: ConfigTelemetry,
    /// Dash0 OTLP endpoint from environment
    pub otlp_endpoint: Option<String>,
    /// Dash0 authentication token from environment
    pub auth_token: Option<String>,
    /// Log level filter
    pub log_level: String,
}

impl TelemetrySetup {
    /// Create telemetry setup from config and environment
    pub fn from_config(
        service_name: &str,
        config: &ConfigTelemetry,
        log_level: &str,
    ) -> Result<Self> {
        // Only check for Dash0 credentials if telemetry is enabled
        let (otlp_endpoint, auth_token) = if config.enabled {
            match (
                std::env::var("DASH0_OTLP_ENDPOINT").ok(),
                std::env::var("DASH0_AUTH_TOKEN").ok(),
            ) {
                (Some(endpoint), Some(token)) => (Some(endpoint), Some(token)),
                _ => {
                    tracing::warn!(
                        "Dash0 credentials not found in environment, telemetry will be disabled"
                    );
                    (None, None)
                }
            }
        } else {
            (None, None)
        };

        Ok(Self {
            service_name: service_name.to_string(),
            config: config.clone(),
            otlp_endpoint,
            auth_token,
            log_level: log_level.to_string(),
        })
    }

    /// Check if Dash0 telemetry is available
    pub fn is_dash0_available(&self) -> bool {
        self.config.enabled && self.otlp_endpoint.is_some() && self.auth_token.is_some()
    }
}

/// Telemetry guard that ensures proper shutdown on drop
pub struct TelemetryGuard {
    _meter_provider: Option<SdkMeterProvider>,
}

impl Drop for TelemetryGuard {
    fn drop(&mut self) {
        info!("Flushing and shutting down telemetry providers...");

        // Force flush any pending traces before shutdown
        use opentelemetry::global;
        use opentelemetry::trace::{Span, Tracer};
        let tracer = global::tracer("shutdown_flush");
        let mut span = tracer.start("shutdown_flush_span");
        span.end();

        global::shutdown_tracer_provider();
        if let Some(meter_provider) = self._meter_provider.take() {
            if let Err(e) = meter_provider.shutdown() {
                eprintln!("Error shutting down meter provider: {}", e);
            }
        }
        info!("Telemetry providers shut down complete");
    }
}

/// Initialize complete telemetry stack
pub fn init_telemetry(setup: TelemetrySetup) -> Result<TelemetryGuard> {
    if !setup.is_dash0_available() {
        // Fallback to simple tracing without OpenTelemetry
        info!("Initializing basic tracing (Dash0 not configured)");
        init_basic_tracing(&setup.log_level)?;
        return Ok(TelemetryGuard {
            _meter_provider: None,
        });
    }

    info!(
        "Initializing Dash0 telemetry for service: {} v{} in {}",
        setup.service_name, setup.config.service_version, setup.config.environment
    );

    // Build resource with all attributes following semantic conventions schema 1.36.0
    let mut resource_attrs = vec![
        KeyValue::new(SERVICE_NAME, setup.service_name.clone()),
        KeyValue::new(SERVICE_VERSION, setup.config.service_version.clone()),
        KeyValue::new("deployment.environment", setup.config.environment.clone()),
        KeyValue::new("telemetry.sdk.language", "rust"),
        KeyValue::new("telemetry.sdk.name", "opentelemetry"),
        KeyValue::new("telemetry.sdk.version", env!("CARGO_PKG_VERSION")),
        KeyValue::new("telemetry.destination", "dash0"), // Custom attribute for Dash0
    ];

    // Add custom resource attributes from config
    for (key, value) in &setup.config.resource_attributes {
        resource_attrs.push(KeyValue::new(key.clone(), value.clone()));
    }

    let resource = Resource::new(resource_attrs);

    // Setup trace context propagator
    global::set_text_map_propagator(TraceContextPropagator::new());

    let otlp_endpoint = setup.otlp_endpoint.as_ref().unwrap();
    let auth_token = setup.auth_token.as_ref().unwrap();

    // Common headers for authentication
    let headers = vec![(
        "Authorization".to_string(),
        format!("Bearer {}", auth_token),
    )];

    // Initialize tracing if enabled
    let tracer_provider = if setup.config.enable_traces {
        info!("Initializing tracing exporter...");
        Some(init_tracer(
            resource.clone(),
            otlp_endpoint.clone(),
            headers.clone(),
            &setup.config,
        )?)
    } else {
        info!("Tracing is disabled in config");
        None
    };

    // Initialize metrics if enabled
    let meter_provider = if setup.config.enable_metrics {
        Some(init_metrics(
            resource.clone(),
            otlp_endpoint.clone(),
            headers.clone(),
            &setup.config,
        )?)
    } else {
        None
    };

    // Initialize logging if enabled
    let _logger_provider = if setup.config.enable_logs {
        Some(init_logger(
            resource.clone(),
            otlp_endpoint.clone(),
            headers.clone(),
            &setup.config,
        )?)
    } else {
        None
    };

    // Setup enhanced log filtering to exclude noisy infrastructure logs
    let env_filter = EnvFilter::try_new(&setup.log_level)
        .context("Invalid log level filter")?
        // Filter out noisy HTTP connection pool debug logs
        .add_directive("hyper_util::client::legacy::pool=info".parse()?)
        .add_directive("reqwest::connect=info".parse()?)
        .add_directive("hyper_util::client::legacy::connect=info".parse()?)
        // Filter out OpenTelemetry internal debug logs
        .add_directive("opentelemetry_sdk=info".parse()?)
        .add_directive("opentelemetry_otlp=info".parse()?)
        .add_directive("opentelemetry_appender_tracing=info".parse()?)
        // Disable SeaORM/SQLx query logs to reduce log volume (Railway rate limiting)
        .add_directive("sqlx::query=warn".parse()?)
        .add_directive("sea_orm::database=warn".parse()?)
        .add_directive("sea_orm=warn".parse()?)
        // Reduce other potentially noisy logs
        .add_directive("tokio_util=info".parse()?)
        .add_directive("tower=info".parse()?)
        .add_directive("h2=info".parse()?)
        // Keep application logs at the configured level
        .add_directive(
            format!(
                "{}={}",
                setup.service_name.replace("-", "_"),
                setup.log_level
            )
            .parse()?,
        )
        .add_directive(format!("api={}", setup.log_level).parse()?)
        .add_directive(format!("common={}", setup.log_level).parse()?)
        .add_directive(format!("db_core={}", setup.log_level).parse()?);
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_ids(true)
        .with_thread_names(true)
        .json();

    // Build the subscriber with all layers
    if let Some(provider) = &tracer_provider {
        let telemetry_layer = OpenTelemetryLayer::new(provider.tracer("tracing"));

        // Add log appender layer if logging is enabled
        if let Some(logger_provider) = &_logger_provider {
            // Create log layer that sends structured logs to OpenTelemetry
            let log_layer = OpenTelemetryTracingBridge::new(logger_provider);

            let subscriber = Registry::default()
                .with(env_filter)
                .with(fmt_layer)
                .with(telemetry_layer)
                .with(log_layer);
            subscriber
                .try_init()
                .context("Failed to initialize tracing subscriber")?;
        } else {
            let subscriber = Registry::default()
                .with(env_filter)
                .with(fmt_layer)
                .with(telemetry_layer);
            subscriber
                .try_init()
                .context("Failed to initialize tracing subscriber")?;
        }
    } else {
        let subscriber = Registry::default().with(env_filter).with(fmt_layer);
        subscriber
            .try_init()
            .context("Failed to initialize tracing subscriber")?;
    }

    info!("Dash0 telemetry initialization complete");

    Ok(TelemetryGuard {
        _meter_provider: meter_provider,
    })
}

/// Initialize basic tracing without OpenTelemetry
pub fn init_basic_tracing(log_level: &str) -> Result<()> {
    let env_filter = EnvFilter::try_new(log_level)
        .context("Invalid log level filter")?
        // Reduce database logging to prevent rate limiting
        .add_directive("sqlx::query=warn".parse()?)
        .add_directive("sea_orm::database=warn".parse()?)
        .add_directive("sea_orm=warn".parse()?)
        // Reduce other potentially noisy logs
        .add_directive("tokio_util=info".parse()?)
        .add_directive("tower=info".parse()?)
        .add_directive("h2=info".parse()?);
        
    let subscriber = Registry::default()
        .with(env_filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_thread_ids(false)
                .with_thread_names(false),
        );

    subscriber
        .try_init()
        .context("Failed to initialize tracing subscriber")?;
    Ok(())
}

/// Initialize OpenTelemetry tracer
fn init_tracer(
    resource: Resource,
    endpoint: String,
    headers: Vec<(String, String)>,
    config: &ConfigTelemetry,
) -> Result<trace::TracerProvider> {
    // Set authentication and dataset headers via environment variable for OTLP
    if let Some((_, token)) = headers.first() {
        std::env::set_var(
            "OTEL_EXPORTER_OTLP_TRACES_HEADERS",
            format!("Authorization={},Dash0-Dataset=default", token), // token already includes "Bearer "
        );
    }

    // Use the endpoint with specific path for traces
    let endpoint_url = format!("{}/v1/traces", endpoint);
    info!(
        "Configuring traces exporter with endpoint: {}",
        endpoint_url
    );

    let exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(endpoint_url)
        .with_protocol(Protocol::HttpJson) // Use JSON like your working curl example
        .with_timeout(Duration::from_secs(30))
        .build()
        .context("Failed to create span exporter")?;

    // Configure batch processor using TelemetryConfig (Dash0-friendly defaults in config)
    std::env::set_var(
        "OTEL_BSP_MAX_EXPORT_BATCH_SIZE",
        config.batch_size.to_string(),
    );
    std::env::set_var("OTEL_BSP_MAX_QUEUE_SIZE", config.batch_size.to_string());
    std::env::set_var(
        "OTEL_BSP_SCHEDULE_DELAY",
        (config.export_interval_sec * 1000).to_string(),
    );
    std::env::set_var(
        "OTEL_BSP_EXPORT_TIMEOUT",
        (config.export_timeout_sec * 1000).to_string(),
    );

    // Create batch processor with environment-configured settings
    let batch_processor =
        BatchSpanProcessor::builder(exporter, opentelemetry_sdk::runtime::Tokio).build();

    let provider = trace::TracerProvider::builder()
        .with_resource(resource)
        .with_span_processor(batch_processor)
        .build();

    global::set_tracer_provider(provider.clone());

    info!(
        "Tracer initialized successfully with endpoint: {}/v1/traces",
        endpoint
    );
    Ok(provider)
}

/// Initialize OpenTelemetry metrics
fn init_metrics(
    resource: Resource,
    endpoint: String,
    headers: Vec<(String, String)>,
    config: &ConfigTelemetry,
) -> Result<SdkMeterProvider> {
    // Set authentication and dataset headers via environment variable for OTLP
    if let Some((_, token)) = headers.first() {
        std::env::set_var(
            "OTEL_EXPORTER_OTLP_METRICS_HEADERS",
            format!("Authorization={},Dash0-Dataset=default", token), // token already includes "Bearer "
        );
    }

    // Use the endpoint with specific path for metrics
    let endpoint_url = format!("{}/v1/metrics", endpoint);
    info!(
        "Configuring metrics exporter with endpoint: {}",
        endpoint_url
    );

    let exporter = MetricExporter::builder()
        .with_http()
        .with_endpoint(endpoint_url)
        .with_protocol(Protocol::HttpJson) // Use JSON like your working curl example
        .with_timeout(Duration::from_secs(30))
        .build()
        .context("Failed to create metrics exporter")?;

    // Configure metric export based on TelemetryConfig (Dash0-friendly defaults in config)
    std::env::set_var(
        "OTEL_METRIC_EXPORT_INTERVAL",
        (config.export_interval_sec * 1000).to_string(),
    );
    std::env::set_var(
        "OTEL_METRIC_EXPORT_TIMEOUT",
        (config.export_timeout_sec * 1000).to_string(),
    );

    let reader = PeriodicReader::builder(exporter, opentelemetry_sdk::runtime::Tokio)
        .with_interval(Duration::from_secs(config.export_interval_sec))
        .with_timeout(Duration::from_secs(config.export_timeout_sec))
        .build();

    let provider = MeterProviderBuilder::default()
        .with_resource(resource)
        .with_reader(reader)
        .build();

    global::set_meter_provider(provider.clone());

    info!("Metrics provider initialized successfully");
    Ok(provider)
}

/// Initialize OpenTelemetry logger
fn init_logger(
    resource: Resource,
    endpoint: String,
    headers: Vec<(String, String)>,
    _config: &ConfigTelemetry,
) -> Result<LoggerProvider> {
    // Set authentication and dataset headers via environment variable for OTLP
    if let Some((_, token)) = headers.first() {
        std::env::set_var(
            "OTEL_EXPORTER_OTLP_LOGS_HEADERS",
            format!("Authorization={},Dash0-Dataset=default", token), // token already includes "Bearer "
        );
    }

    // Use the endpoint with specific path for logs
    let endpoint_url = format!("{}/v1/logs", endpoint);
    info!("Configuring logs exporter with endpoint: {}", endpoint_url);

    let exporter = LogExporter::builder()
        .with_http()
        .with_endpoint(endpoint_url)
        .with_protocol(Protocol::HttpJson) // Use JSON like your working curl example
        .with_timeout(Duration::from_secs(30))
        .build()
        .context("Failed to create log exporter")?;

    let batch_processor =
        BatchLogProcessor::builder(exporter, opentelemetry_sdk::runtime::Tokio).build();

    let provider = LoggerProvider::builder()
        .with_resource(resource)
        .with_log_processor(batch_processor)
        .build();

    info!("Logger provider initialized successfully");
    Ok(provider)
}

/// Helper to create a tracer for a specific component
pub fn tracer(name: &'static str) -> opentelemetry::global::BoxedTracer {
    global::tracer(name)
}

/// Helper to create a meter for a specific component
pub fn meter(name: &'static str) -> opentelemetry::metrics::Meter {
    global::meter(name)
}

/// Record a custom metric value
pub fn record_metric(name: &'static str, value: f64, attributes: &[KeyValue]) {
    let meter = meter("custom_metrics");
    let gauge = meter.f64_gauge(name).build();
    gauge.record(value, attributes);
}

/// Record a counter increment
pub fn increment_counter(name: &'static str, attributes: &[KeyValue]) {
    let meter = meter("custom_metrics");
    let counter = meter.u64_counter(name).build();
    counter.add(1, attributes);
}

/// Create a histogram for recording durations
pub fn duration_histogram(name: &'static str) -> opentelemetry::metrics::Histogram<f64> {
    let meter = meter("custom_metrics");
    meter
        .f64_histogram(name)
        .with_unit("ms")
        .with_description("Duration in milliseconds")
        .build()
}
