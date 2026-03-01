//! Type-safe configuration system with app-specific configurations
//! Supports hierarchical loading from TOML files and environment variables

use crate::errors::ConfigError;
use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};

/// Provide a sanitized representation of configuration for safe logging.
/// Secret-bearing fields are simply omitted instead of redacted.
pub trait ToLog {
    fn to_log(&self) -> JsonValue;
}

/// Database configuration shared by all applications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    /// Database URL is provided via DATABASE_URL environment variable
    #[serde(skip)]
    pub url: String,
    pub max_connections: Option<u32>,
    pub min_connections: Option<u32>,
    pub connection_timeout: Option<u64>,
    pub acquire_timeout: Option<u64>,
    pub idle_timeout: Option<u64>,
    pub max_lifetime: Option<u64>,
}

impl ToLog for DatabaseConfig {
    fn to_log(&self) -> JsonValue {
        // Omit connection URL entirely from logs
        json!({
            "max_connections": self.max_connections,
            "min_connections": self.min_connections,
            "connection_timeout": self.connection_timeout,
            "acquire_timeout": self.acquire_timeout,
            "idle_timeout": self.idle_timeout,
            "max_lifetime": self.max_lifetime,
        })
    }
}

/// Server configuration for API service only
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

impl ToLog for ServerConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "host": self.host,
            "port": self.port,
        })
    }
}

/// Logging configuration per application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: Option<String>,
}

impl ToLog for LoggingConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "level": self.level,
            "format": self.format,
        })
    }
}

/// Telemetry configuration for internal metrics and tracing behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub service_version: String,
    pub environment: String,
    pub enable_traces: bool,
    pub enable_logs: bool,
    pub traces_sample_rate: f32,
    pub event_sample_rate: f32,
    pub shutdown_timeout_sec: u64,
    pub send_default_pii: bool,
    pub tags: std::collections::HashMap<String, String>,
}

impl ToLog for TelemetryConfig {
    fn to_log(&self) -> JsonValue {
        // Omit tags to avoid leaking sensitive metadata via logs
        json!({
            "enabled": self.enabled,
            "service_version": self.service_version,
            "environment": self.environment,
            "enable_traces": self.enable_traces,
            "enable_logs": self.enable_logs,
            "traces_sample_rate": self.traces_sample_rate,
            "event_sample_rate": self.event_sample_rate,
            "shutdown_timeout_sec": self.shutdown_timeout_sec,
            "send_default_pii": self.send_default_pii,
        })
    }
}

/// API-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub database: DatabaseConfig,
    pub server: ServerConfig,
    pub logging: LoggingConfig,
    pub telemetry: TelemetryConfig,
}

impl ToLog for ApiConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "database": self.database.to_log(),
            "server": self.server.to_log(),
            "logging": self.logging.to_log(),
            "telemetry": self.telemetry.to_log(),
        })
    }
}

/// Configuration for Discourse indexing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscourseConfig {
    pub instances: Vec<DiscourseInstanceConfig>,
    #[serde(default = "default_full_refresh_interval_sec")]
    pub full_refresh_interval_sec: u64,
    #[serde(default = "default_recent_update_interval_sec")]
    pub recent_update_interval_sec: u64,
    #[serde(default = "default_max_pages_per_run")]
    pub max_pages_per_run: u32,
    #[serde(default = "default_recent_lookback_hours")]
    pub recent_lookback_hours: i64,
    #[serde(default = "default_http_max_concurrency")]
    pub http_max_concurrency: usize,
    #[serde(default = "default_max_post_tasks")]
    pub max_post_tasks: usize,
    #[serde(default = "default_likes_concurrency")]
    pub likes_concurrency: usize,
    #[serde(default = "default_revisions_concurrency")]
    pub revisions_concurrency: usize,
    #[serde(default = "default_recent_topic_pages_cap")]
    pub recent_topic_pages_cap: u32,
    #[serde(default = "default_metrics_refresh_batch_size")]
    pub metrics_refresh_batch_size: u32,
    #[serde(default = "default_metrics_active_window_days")]
    pub metrics_active_window_days: i64,
    #[serde(default = "default_metrics_active_refresh_hours")]
    pub metrics_active_refresh_hours: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscourseInstanceConfig {
    pub key: String,
    pub base_url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    // Per-instance tuning (optional)
    #[serde(default)]
    pub http_max_concurrency: Option<usize>,
    #[serde(default)]
    pub max_post_tasks: Option<usize>,
    #[serde(default)]
    pub likes_concurrency: Option<usize>,
    #[serde(default)]
    pub revisions_concurrency: Option<usize>,
    // Recent caps per instance
    #[serde(default)]
    pub recent_topic_pages_cap: Option<u32>,
    // Optional per-instance scheduling overrides
    #[serde(default)]
    pub full_refresh_interval_sec: Option<u64>,
    #[serde(default)]
    pub recent_update_interval_sec: Option<u64>,
    // Metrics refresh tuning (optional)
    #[serde(default)]
    pub metrics_refresh_batch_size: Option<u32>,
    #[serde(default)]
    pub metrics_active_window_days: Option<i64>,
    #[serde(default)]
    pub metrics_active_refresh_hours: Option<u64>,
}

fn default_true() -> bool {
    true
}
fn default_full_refresh_interval_sec() -> u64 {
    24 * 60 * 60
}
fn default_recent_update_interval_sec() -> u64 {
    15 * 60
}
fn default_max_pages_per_run() -> u32 {
    1000
}
fn default_recent_lookback_hours() -> i64 {
    2
}
fn default_http_max_concurrency() -> usize {
    4
}
fn default_max_post_tasks() -> usize {
    3
}
fn default_likes_concurrency() -> usize {
    2
}
fn default_revisions_concurrency() -> usize {
    1
}
fn default_recent_topic_pages_cap() -> u32 {
    1
}
fn default_metrics_refresh_batch_size() -> u32 {
    30
}
fn default_metrics_active_window_days() -> i64 {
    30
}
fn default_metrics_active_refresh_hours() -> u64 {
    8
}

impl ToLog for DiscourseConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "instances": self.instances.iter().map(|i| json!({
                "key": i.key,
                "base_url": i.base_url,
                "enabled": i.enabled,
                "http_max_concurrency": i.http_max_concurrency,
                "max_post_tasks": i.max_post_tasks,
                "likes_concurrency": i.likes_concurrency,
                "revisions_concurrency": i.revisions_concurrency,
                "recent_topic_pages_cap": i.recent_topic_pages_cap,
                "full_refresh_interval_sec": i.full_refresh_interval_sec,
                "recent_update_interval_sec": i.recent_update_interval_sec,
                "metrics_refresh_batch_size": i.metrics_refresh_batch_size,
                "metrics_active_window_days": i.metrics_active_window_days,
                "metrics_active_refresh_hours": i.metrics_active_refresh_hours,
            })).collect::<Vec<_>>(),
            "full_refresh_interval_sec": self.full_refresh_interval_sec,
            "recent_update_interval_sec": self.recent_update_interval_sec,
            "max_pages_per_run": self.max_pages_per_run,
            "recent_lookback_hours": self.recent_lookback_hours,
            "http_max_concurrency": self.http_max_concurrency,
            "max_post_tasks": self.max_post_tasks,
            "likes_concurrency": self.likes_concurrency,
            "revisions_concurrency": self.revisions_concurrency,
            "recent_topic_pages_cap": self.recent_topic_pages_cap,
            "metrics_refresh_batch_size": self.metrics_refresh_batch_size,
            "metrics_active_window_days": self.metrics_active_window_days,
            "metrics_active_refresh_hours": self.metrics_active_refresh_hours,
        })
    }
}

/// Indexer-specific configuration (no server needed)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexerConfig {
    pub database: DatabaseConfig,
    pub logging: LoggingConfig,
    pub telemetry: TelemetryConfig,
    pub discourse: Option<DiscourseConfig>,
    /// NEAR Lake / chain streaming configuration for NEAR indexers
    #[serde(default)]
    pub near: Option<NearLakeConfig>,
}

impl ToLog for IndexerConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "database": self.database.to_log(),
            "logging": self.logging.to_log(),
            "telemetry": self.telemetry.to_log(),
            "discourse": self.discourse.as_ref().map(|d| d.to_log()),
            "near": self.near.as_ref().map(|n| n.to_log()),
        })
    }
}

/// NEAR Lake / chain streaming configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NearLakeConfig {
    /// Stream data provider: "lake" (AWS S3) or "fastnear" (HTTP API)
    #[serde(default)]
    pub provider: NearStreamProvider,
    /// Chain to index: "mainnet" or "testnet"
    #[serde(default = "default_chain")]
    pub chain: String,
    /// Start block height for backfill
    #[serde(default)]
    pub start_height: u64,
    /// Optional concurrency hint for internal batching (not all frameworks use it)
    #[serde(default)]
    pub concurrency: Option<usize>,
    /// Optional preload pool size for NEAR Lake internal prefetching
    /// Larger values can increase memory usage due to buffered blocks in the upstream channel.
    /// If not set, the indexer will default to a conservative small value.
    #[serde(default)]
    pub preload_pool_size: Option<usize>,
    /// Optional FastNEAR Bearer token for paid/private access.
    /// Not required for public neardata endpoints.
    #[serde(default)]
    pub fastnear_api_key: Option<String>,
    /// Optional number of fetch threads for FastNEAR provider.
    /// If not set, the indexer applies a conservative default.
    #[serde(default)]
    pub fastnear_num_threads: Option<u64>,
    /// Module-specific configurations
    #[serde(default)]
    pub modules: Option<NearModulesConfig>,
}

/// Stream provider for NEAR indexers.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum NearStreamProvider {
    /// S3-backed NEAR Lake stream.
    #[default]
    Lake,
    /// FastNEAR HTTP-backed stream.
    Fastnear,
}

impl NearStreamProvider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lake => "lake",
            Self::Fastnear => "fastnear",
        }
    }
}

/// Stream assignment mode for NEAR modules.
///
/// `shared` means the module consumes from the shared stream with other shared modules.
/// `independent` means the module runs on its own dedicated stream.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum NearModuleStreamMode {
    #[default]
    Shared,
    Independent,
}

impl NearModuleStreamMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Shared => "shared",
            Self::Independent => "independent",
        }
    }
}

/// NEAR indexer modules configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NearModulesConfig {
    /// Raw data module configuration
    #[serde(default)]
    pub raw: Option<RawModuleConfig>,
    /// Accounts module configuration
    #[serde(default)]
    pub accounts: Option<AccountsModuleConfig>,
    /// Tokens module configuration
    #[serde(default)]
    pub tokens: Option<TokensModuleConfig>,
    /// Staking pools module configuration
    #[serde(default)]
    pub staking_pools: Option<StakingPoolsModuleConfig>,
    /// veNEAR governance module configuration
    #[serde(default)]
    pub venear_governance: Option<VenearGovernanceModuleConfig>,
}

/// Raw data module configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawModuleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub stream_mode: NearModuleStreamMode,
    #[serde(default)]
    pub start_height: Option<u64>,
}

/// Accounts module configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountsModuleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub stream_mode: NearModuleStreamMode,
    #[serde(default)]
    pub start_height: Option<u64>,
}

/// Tokens module configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokensModuleConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub stream_mode: NearModuleStreamMode,
    #[serde(default)]
    pub start_height: Option<u64>,
    #[serde(default = "default_true")]
    pub track_nep141: bool,
    #[serde(default = "default_true")]
    pub track_nep171: bool,
    #[serde(default = "default_true")]
    pub track_nep245: bool,
    #[serde(default)]
    pub contracts: Vec<String>,
}

/// Staking pools module configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StakingPoolsModuleConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub stream_mode: NearModuleStreamMode,
    #[serde(default)]
    pub start_height: Option<u64>,
    #[serde(default = "default_pool_patterns")]
    pub pool_patterns: Vec<String>,
    #[serde(default)]
    pub custom_pools: Vec<String>,
    #[serde(default = "default_true")]
    pub track_epochs: bool,
    #[serde(default = "default_true")]
    pub track_delegators: bool,
    #[serde(default = "default_true")]
    pub parse_events: bool,
}

/// veNEAR governance module configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VenearGovernanceModuleConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub stream_mode: NearModuleStreamMode,
    #[serde(default)]
    pub start_height: Option<u64>,
    #[serde(default = "default_venear_contract_id")]
    pub venear_contract_id: String,
    #[serde(default = "default_voting_contract_id")]
    pub voting_contract_id: String,
    #[serde(default = "default_lockup_account_suffix")]
    pub lockup_account_suffix: String,
    #[serde(default)]
    pub known_lockups: Vec<String>,
}

fn default_chain() -> String {
    "mainnet".to_string()
}

fn default_pool_patterns() -> Vec<String> {
    vec!["*.poolv1.near".to_string(), "*.pool.near".to_string()]
}

fn default_venear_contract_id() -> String {
    "venear.dao".to_string()
}

fn default_voting_contract_id() -> String {
    "vote.dao".to_string()
}

fn default_lockup_account_suffix() -> String {
    ".venear.dao".to_string()
}

impl ToLog for NearLakeConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "provider": self.provider.as_str(),
            "chain": self.chain,
            "start_height": self.start_height,
            "concurrency": self.concurrency,
            "preload_pool_size": self.preload_pool_size,
            "fastnear_api_key_set": self.fastnear_api_key.is_some(),
            "fastnear_num_threads": self.fastnear_num_threads,
            "modules": self.modules.as_ref().map(|m| m.to_log()),
        })
    }
}

impl ToLog for NearModulesConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "raw": self.raw.as_ref().map(|r| r.to_log()),
            "accounts": self.accounts.as_ref().map(|a| a.to_log()),
            "tokens": self.tokens.as_ref().map(|t| t.to_log()),
            "staking_pools": self.staking_pools.as_ref().map(|s| s.to_log()),
            "venear_governance": self.venear_governance.as_ref().map(|v| v.to_log()),
        })
    }
}

impl ToLog for RawModuleConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "enabled": self.enabled,
            "stream_mode": self.stream_mode.as_str(),
            "start_height": self.start_height,
        })
    }
}

impl ToLog for AccountsModuleConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "enabled": self.enabled,
            "stream_mode": self.stream_mode.as_str(),
            "start_height": self.start_height,
        })
    }
}

impl ToLog for TokensModuleConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "enabled": self.enabled,
            "stream_mode": self.stream_mode.as_str(),
            "start_height": self.start_height,
            "track_nep141": self.track_nep141,
            "track_nep171": self.track_nep171,
            "track_nep245": self.track_nep245,
            "contracts": if self.contracts.is_empty() {
                "all".to_string()
            } else {
                format!("{} contracts", self.contracts.len())
            },
        })
    }
}

impl ToLog for StakingPoolsModuleConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "enabled": self.enabled,
            "stream_mode": self.stream_mode.as_str(),
            "start_height": self.start_height,
            "pool_patterns": self.pool_patterns,
            "custom_pools": if self.custom_pools.is_empty() {
                "none".to_string()
            } else {
                format!("{} custom pools", self.custom_pools.len())
            },
            "track_epochs": self.track_epochs,
            "track_delegators": self.track_delegators,
            "parse_events": self.parse_events,
        })
    }
}

impl ToLog for VenearGovernanceModuleConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "enabled": self.enabled,
            "stream_mode": self.stream_mode.as_str(),
            "start_height": self.start_height,
            "venear_contract_id": self.venear_contract_id,
            "voting_contract_id": self.voting_contract_id,
            "lockup_account_suffix": self.lockup_account_suffix,
            "known_lockups": if self.known_lockups.is_empty() {
                "none".to_string()
            } else {
                format!("{} lockups", self.known_lockups.len())
            },
        })
    }
}

/// Unified configuration enum for type safety
#[derive(Debug, Clone)]
pub enum AppConfig {
    Api(Box<ApiConfig>),
    Indexer(Box<IndexerConfig>),
}

impl ToLog for AppConfig {
    fn to_log(&self) -> JsonValue {
        match self {
            AppConfig::Api(cfg) => json!({
                "kind": "api",
                "config": cfg.as_ref().to_log()
            }),
            AppConfig::Indexer(cfg) => json!({
                "kind": "indexer",
                "config": cfg.as_ref().to_log()
            }),
        }
    }
}

impl AppConfig {
    /// Load API configuration
    pub fn load_api() -> Result<Self, ConfigError> {
        let config = ConfigBuilder::api().load_with_database_url()?;
        Ok(AppConfig::Api(Box::new(config)))
    }

    /// Load indexer configuration by name
    pub fn load_indexer(name: &str) -> Result<Self, ConfigError> {
        let config = ConfigBuilder::indexer(name).load_with_database_url()?;
        Ok(AppConfig::Indexer(Box::new(config)))
    }

    /// Get database configuration (available in all app types)
    pub fn database(&self) -> &DatabaseConfig {
        match self {
            AppConfig::Api(config) => {
                let config: &ApiConfig = config;
                &config.database
            }
            AppConfig::Indexer(config) => {
                let config: &IndexerConfig = config;
                &config.database
            }
        }
    }

    /// Get logging configuration (available in all app types)
    pub fn logging(&self) -> &LoggingConfig {
        match self {
            AppConfig::Api(config) => {
                let config: &ApiConfig = config;
                &config.logging
            }
            AppConfig::Indexer(config) => {
                let config: &IndexerConfig = config;
                &config.logging
            }
        }
    }

    /// Get telemetry configuration (available in all app types)
    pub fn telemetry(&self) -> &TelemetryConfig {
        match self {
            AppConfig::Api(config) => {
                let config: &ApiConfig = config;
                &config.telemetry
            }
            AppConfig::Indexer(config) => {
                let config: &IndexerConfig = config;
                &config.telemetry
            }
        }
    }

    /// Get Discourse indexing configuration if present (indexer only)
    pub fn discourse(&self) -> Option<&DiscourseConfig> {
        match self {
            AppConfig::Api(_) => None,
            AppConfig::Indexer(config) => {
                let config: &IndexerConfig = config;
                config.discourse.as_ref()
            }
        }
    }

    /// Get server configuration (only available for API)
    pub fn server(&self) -> Option<&ServerConfig> {
        match self {
            AppConfig::Api(config) => {
                let config: &ApiConfig = config;
                Some(&config.server)
            }
            AppConfig::Indexer(_) => None,
        }
    }

    /// Get server configuration (panics if not API)
    pub fn server_required(&self) -> &ServerConfig {
        match self {
            AppConfig::Api(config) => {
                let config: &ApiConfig = config;
                &config.server
            }
            AppConfig::Indexer(_) => panic!("Server configuration not available for indexer"),
        }
    }
}

/// Configuration builder for different app types
pub struct ConfigBuilder;

impl ConfigBuilder {
    /// Load API configuration with server + database + logging
    pub fn api() -> ConfigLoader<ApiConfig> {
        ConfigLoader::new("api/config.toml")
    }

    /// Load indexer configuration with database + logging only
    pub fn indexer(name: &str) -> ConfigLoader<IndexerConfig> {
        let container_path = format!("/{}-config.toml", name);
        let host_path = format!("indexers/{}/config.toml", name);

        // Check if we're in a container (look for container-specific path first)
        if std::path::Path::new(&container_path).exists() {
            ConfigLoader::new(&container_path)
        } else {
            ConfigLoader::new(&host_path)
        }
    }
}

/// Type-safe configuration loader
pub struct ConfigLoader<T> {
    binary_config_path: String,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> ConfigLoader<T>
where
    T: for<'de> Deserialize<'de>,
{
    fn new(binary_config_path: &str) -> Self {
        Self {
            binary_config_path: binary_config_path.to_string(),
            _phantom: std::marker::PhantomData,
        }
    }

    /// Load configuration with hierarchical merging
    pub fn load(self) -> Result<T, ConfigError> {
        let mut figment = Figment::new();

        // Check if root config exists and merge it if present (for backward compatibility)
        if std::path::Path::new("config.toml").exists() {
            figment = figment.merge(Toml::file("config.toml"));
        }

        // Add binary-specific config - this is now required since each app has its own config
        let binary_config = std::path::Path::new(&self.binary_config_path);
        if binary_config.exists() {
            figment = figment.merge(Toml::file(&self.binary_config_path));
        } else {
            return Err(ConfigError::ParseError {
                message: format!(
                    "Required config file not found: {}",
                    self.binary_config_path
                ),
            });
        }

        // Environment variables have highest priority.
        // Use double underscore to address nested keys, e.g., APP_SERVER__HOST
        figment = figment.merge(Env::prefixed("APP_").split("__"));

        let config: T = figment.extract().map_err(|e| ConfigError::ParseError {
            message: e.to_string(),
        })?;

        Ok(config)
    }

    /// Load configuration strictly from files (no environment overrides)
    pub fn load_no_env(self) -> Result<T, ConfigError> {
        let mut figment = Figment::new();

        // Check if root config exists and merge it if present (for backward compatibility)
        if std::path::Path::new("config.toml").exists() {
            figment = figment.merge(Toml::file("config.toml"));
        }

        // Add binary-specific config - this is now required since each app has its own config
        let binary_config = std::path::Path::new(&self.binary_config_path);
        if binary_config.exists() {
            figment = figment.merge(Toml::file(&self.binary_config_path));
        } else {
            return Err(ConfigError::ParseError {
                message: format!(
                    "Required config file not found: {}",
                    self.binary_config_path
                ),
            });
        }

        let config: T = figment.extract().map_err(|e| ConfigError::ParseError {
            message: e.to_string(),
        })?;
        Ok(config)
    }
}

impl ConfigLoader<ApiConfig> {
    /// Load API configuration and inject DATABASE_URL and PORT
    pub fn load_with_database_url(self) -> Result<ApiConfig, ConfigError> {
        let mut config = self.load()?;

        // Load DATABASE_URL from environment
        let database_url =
            std::env::var("DATABASE_URL").map_err(|_| ConfigError::MissingEnvVar {
                var_name: "DATABASE_URL".to_string(),
            })?;
        config.database.url = database_url;

        // Load PORT from environment (Railway requires this for proper health checks)
        if let Ok(port_str) = std::env::var("PORT") {
            if let Ok(port) = port_str.parse::<u16>() {
                config.server.port = port;
                tracing::info!("Using PORT from environment: {}", port);
            }
        }

        Ok(config)
    }
}

impl ConfigLoader<IndexerConfig> {
    /// Load indexer configuration and inject DATABASE_URL
    pub fn load_with_database_url(self) -> Result<IndexerConfig, ConfigError> {
        let mut config = self.load()?;

        // Load DATABASE_URL from environment
        let database_url =
            std::env::var("DATABASE_URL").map_err(|_| ConfigError::MissingEnvVar {
                var_name: "DATABASE_URL".to_string(),
            })?;
        config.database.url = database_url;

        Ok(config)
    }
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            // URL will be loaded from DATABASE_URL environment variable
            url: String::new(),
            // Much more aggressive settings for MySQL connection pool issues
            // MySQL has known issues with connections not being returned to pool
            max_connections: Some(200),   // Increased significantly
            min_connections: Some(10),    // Higher minimum to keep connections ready
            connection_timeout: Some(15), // Reduced from 30 to fail faster
            acquire_timeout: Some(5),     // Very short - fail fast if pool exhausted
            idle_timeout: Some(120),      // Much shorter - 2 minutes instead of 10
            max_lifetime: Some(300),      // Much shorter - 5 minutes to force connection recycling
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: Some("json".to_string()),
        }
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            service_version: "0.1.0".to_string(),
            environment: "development".to_string(),
            enable_traces: true,
            enable_logs: true,
            traces_sample_rate: 1.0,
            event_sample_rate: 1.0,
            shutdown_timeout_sec: 2,
            send_default_pii: true,
            tags: std::collections::HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;

    // Guard to avoid concurrent chdir side effects in this module
    static CWD_GUARD: Mutex<()> = Mutex::new(());

    fn lock_cwd() -> std::sync::MutexGuard<'static, ()> {
        match CWD_GUARD.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    #[test]
    fn load_api_respects_env_overrides() {
        let _g = lock_cwd();

        // Create an isolated temporary directory structure with empty config files
        let base = std::env::temp_dir().join(format!(
            "config_test_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        fs::create_dir_all(base.join("api")).unwrap();
        // Root workspace config: provide database section (url comes from env)
        let root_cfg = r#"
            [database]
            max_connections = 5
            min_connections = 1
            connection_timeout = 30
            acquire_timeout = 30
            idle_timeout = 600
            max_lifetime = 1800
        "#;
        fs::write(base.join("config.toml"), root_cfg).unwrap();

        // API binary config: server/logging/telemetry sections
        let api_cfg = r#"
            [server]
            host = "127.0.0.1"
            port = 3000

            [logging]
            level = "info"
            format = "json"

            [telemetry]
            enabled = true
            service_version = "0.1.0"
            environment = "development"
            enable_traces = true
            enable_logs = true
            traces_sample_rate = 1.0
            event_sample_rate = 1.0
            shutdown_timeout_sec = 2
            send_default_pii = true
            tags = {}
        "#;
        fs::write(base.join("api").join("config.toml"), api_cfg).unwrap();

        // Change CWD for this test only
        let old_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&base).unwrap();

        // Set env vars
        std::env::set_var("DATABASE_URL", "mysql://user:pass@localhost/db");
        // Figment nested keys via environment typically use double underscore
        std::env::set_var("APP_SERVER__HOST", "0.0.0.0");
        // Port may be overridden by Railway-style PORT env var in load_with_database_url
        std::env::set_var("PORT", "7777");

        // Load config via builder
        let app_cfg = AppConfig::load_api().expect("config should load");

        match app_cfg {
            AppConfig::Api(cfg) => {
                let cfg = cfg.as_ref();
                assert_eq!(cfg.database.url, "mysql://user:pass@localhost/db");
                assert_eq!(cfg.server.host, "0.0.0.0");
                assert_eq!(cfg.server.port, 7777);
            }
            _ => panic!("Expected Api config"),
        }

        // Cleanup
        std::env::set_current_dir(&old_cwd).unwrap();
        std::env::remove_var("DATABASE_URL");
        std::env::remove_var("APP_SERVER__HOST");
        std::env::remove_var("PORT");

        // Best-effort remove temp dir
        let _ = fs::remove_file(base.join("config.toml"));
        let _ = fs::remove_file(base.join("api").join("config.toml"));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn load_indexer_respects_env_overrides_and_host_config() {
        let _g = lock_cwd();

        // Create temp workspace layout
        let base = std::env::temp_dir().join(format!(
            "config_test_indexer_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        fs::create_dir_all(base.join("indexers").join("indexer-1")).unwrap();

        // Root config provides [database] defaults
        let root_cfg = r#"
            [database]
            max_connections = 10
            min_connections = 1
            connection_timeout = 30
            acquire_timeout = 30
            idle_timeout = 600
            max_lifetime = 1800
        "#;
        fs::write(base.join("config.toml"), root_cfg).unwrap();

        // Indexer-specific config provides logging/telemetry
        let idx_cfg = r#"
            [logging]
            level = "debug"

            [telemetry]
            enabled = true
            service_version = "0.1.0"
            environment = "development"
            enable_traces = true
            enable_logs = true
            traces_sample_rate = 1.0
            event_sample_rate = 1.0
            shutdown_timeout_sec = 2
            send_default_pii = true
            tags = {}
        "#;
        fs::write(
            base.join("indexers").join("indexer-1").join("config.toml"),
            idx_cfg,
        )
        .unwrap();

        let old_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&base).unwrap();

        // Env overrides
        std::env::set_var("DATABASE_URL", "mysql://user:pass@localhost/idx");
        std::env::set_var("APP_TELEMETRY__ENABLED", "false");

        let app_cfg = AppConfig::load_indexer("indexer-1").expect("indexer config loads");
        match app_cfg {
            AppConfig::Indexer(cfg) => {
                let cfg = cfg.as_ref();
                // DATABASE_URL injected
                assert_eq!(cfg.database.url, "mysql://user:pass@localhost/idx");
                // Host config loaded
                assert_eq!(cfg.logging.level, "debug");
                // Env override applied
                assert!(!cfg.telemetry.enabled);
            }
            _ => panic!("Expected Indexer config"),
        }

        // Cleanup
        std::env::set_current_dir(&old_cwd).unwrap();
        std::env::remove_var("DATABASE_URL");
        std::env::remove_var("APP_TELEMETRY__ENABLED");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn server_is_none_for_indexer() {
        let _g = lock_cwd();

        // Temp workspace with root + indexer config
        let base = std::env::temp_dir().join(format!(
            "config_test_indexer_server_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        fs::create_dir_all(base.join("indexers").join("indexer-1")).unwrap();
        let root_cfg = r#"
            [database]
            max_connections = 5
            min_connections = 1
            connection_timeout = 30
            acquire_timeout = 30
            idle_timeout = 600
            max_lifetime = 1800
        "#;
        fs::write(base.join("config.toml"), root_cfg).unwrap();
        let idx_cfg = r#"
            [logging]
            level = "info"

            [telemetry]
            enabled = true
            service_version = "0.1.0"
            environment = "development"
            enable_traces = true
            enable_logs = true
            traces_sample_rate = 1.0
            event_sample_rate = 1.0
            shutdown_timeout_sec = 2
            send_default_pii = true
            tags = {}
        "#;
        fs::write(base.join("indexers/indexer-1/config.toml"), idx_cfg).unwrap();

        let old_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&base).unwrap();
        std::env::set_var("DATABASE_URL", "mysql://user:pass@localhost/panic");

        let app_cfg = AppConfig::load_indexer("indexer-1").unwrap();
        assert!(app_cfg.server().is_none());

        // Cleanup
        std::env::set_current_dir(&old_cwd).unwrap();
        std::env::remove_var("DATABASE_URL");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn near_module_stream_mode_defaults_to_shared() {
        assert_eq!(
            NearModuleStreamMode::default(),
            NearModuleStreamMode::Shared
        );

        let near_cfg = NearLakeConfig::default();
        assert_eq!(near_cfg.provider, NearStreamProvider::Lake);
        assert_eq!(near_cfg.fastnear_num_threads, None);

        let raw = RawModuleConfig::default();
        let accounts = AccountsModuleConfig::default();
        let tokens = TokensModuleConfig::default();
        let staking_pools = StakingPoolsModuleConfig::default();
        let venear = VenearGovernanceModuleConfig::default();

        assert_eq!(raw.stream_mode, NearModuleStreamMode::Shared);
        assert_eq!(accounts.stream_mode, NearModuleStreamMode::Shared);
        assert_eq!(tokens.stream_mode, NearModuleStreamMode::Shared);
        assert_eq!(staking_pools.stream_mode, NearModuleStreamMode::Shared);
        assert_eq!(venear.stream_mode, NearModuleStreamMode::Shared);

        assert_eq!(raw.start_height, None);
        assert_eq!(accounts.start_height, None);
        assert_eq!(tokens.start_height, None);
        assert_eq!(staking_pools.start_height, None);
        assert_eq!(venear.start_height, None);
    }

    #[test]
    fn near_stream_provider_deserializes_fastnear() {
        let cfg: NearLakeConfig = serde_json::from_value(serde_json::json!({
            "provider": "fastnear"
        }))
        .expect("provider should deserialize");

        assert_eq!(cfg.provider, NearStreamProvider::Fastnear);
    }

    #[test]
    fn near_module_stream_mode_deserializes_independent() {
        let cfg: VenearGovernanceModuleConfig = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "stream_mode": "independent"
        }))
        .expect("stream mode should deserialize");

        assert_eq!(cfg.stream_mode, NearModuleStreamMode::Independent);
    }

    #[test]
    fn near_module_start_height_deserializes() {
        let cfg: VenearGovernanceModuleConfig = serde_json::from_value(serde_json::json!({
            "enabled": true,
            "start_height": 160000000
        }))
        .expect("start height should deserialize");

        assert_eq!(cfg.start_height, Some(160000000));
    }
}
