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
    pub worker_threads: Option<usize>,
}

impl ToLog for ServerConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "host": self.host,
            "port": self.port,
            "worker_threads": self.worker_threads,
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

/// Telemetry configuration for OpenTelemetry/Dash0 integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    pub enabled: bool,
    pub service_version: String,
    pub environment: String,
    pub enable_metrics: bool,
    pub enable_traces: bool,
    pub enable_logs: bool,
    pub batch_size: u16,
    pub export_timeout_sec: u64,
    pub export_interval_sec: u64,
    pub resource_attributes: std::collections::HashMap<String, String>,
}

impl ToLog for TelemetryConfig {
    fn to_log(&self) -> JsonValue {
        // Omit resource_attributes entirely to avoid leaking any data via logs
        json!({
            "enabled": self.enabled,
            "service_version": self.service_version,
            "environment": self.environment,
            "enable_metrics": self.enable_metrics,
            "enable_traces": self.enable_traces,
            "enable_logs": self.enable_logs,
            "batch_size": self.batch_size,
            "export_timeout_sec": self.export_timeout_sec,
            "export_interval_sec": self.export_interval_sec,
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscourseInstanceConfig {
    pub key: String,
    pub base_url: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    // Per-instance tuning (optional)
    #[serde(default)]
    pub max_http_concurrency: Option<usize>,
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

impl ToLog for DiscourseConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "instances": self.instances.iter().map(|i| json!({
                "key": i.key,
                "base_url": i.base_url,
                "enabled": i.enabled,
            })).collect::<Vec<_>>(),
            "full_refresh_interval_sec": self.full_refresh_interval_sec,
            "recent_update_interval_sec": self.recent_update_interval_sec,
            "max_pages_per_run": self.max_pages_per_run,
            "recent_lookback_hours": self.recent_lookback_hours,
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
    /// Chain to index: "mainnet" or "testnet"
    #[serde(default = "default_chain")]
    pub chain: String,
    /// Start block height for backfill
    #[serde(default)]
    pub start_height: u64,
    /// Optional concurrency hint for internal batching (not all frameworks use it)
    #[serde(default)]
    pub concurrency: Option<usize>,
    /// Optional AWS region override (e.g., eu-central-1). Typically use AWS_REGION env.
    #[serde(default)]
    pub region: Option<String>,
    /// Module-specific configurations
    #[serde(default)]
    pub modules: Option<NearModulesConfig>,
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
}

/// Raw data module configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RawModuleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Accounts module configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountsModuleConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
}

/// Tokens module configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokensModuleConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub track_nep141: bool,
    #[serde(default = "default_true")]
    pub track_nep171: bool,
    #[serde(default = "default_true")]
    pub track_nep245: bool,
    #[serde(default)]
    pub contracts: Vec<String>,
}

fn default_chain() -> String {
    "mainnet".to_string()
}

impl ToLog for NearLakeConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "chain": self.chain,
            "start_height": self.start_height,
            "concurrency": self.concurrency,
            // do not include region if using env; but it's not secret either
            "region": self.region,
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
        })
    }
}

impl ToLog for RawModuleConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "enabled": self.enabled,
        })
    }
}

impl ToLog for AccountsModuleConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "enabled": self.enabled,
        })
    }
}

impl ToLog for TokensModuleConfig {
    fn to_log(&self) -> JsonValue {
        json!({
            "enabled": self.enabled,
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

/// Unified configuration enum for type safety
#[derive(Debug, Clone)]
pub enum AppConfig {
    Api(ApiConfig),
    Indexer(IndexerConfig),
}

impl ToLog for AppConfig {
    fn to_log(&self) -> JsonValue {
        match self {
            AppConfig::Api(cfg) => json!({
                "kind": "api",
                "config": cfg.to_log()
            }),
            AppConfig::Indexer(cfg) => json!({
                "kind": "indexer",
                "config": cfg.to_log()
            }),
        }
    }
}

impl AppConfig {
    /// Load API configuration
    pub fn load_api() -> Result<Self, ConfigError> {
        let config = ConfigBuilder::api().load_with_database_url()?;
        Ok(AppConfig::Api(config))
    }

    /// Load indexer configuration by name
    pub fn load_indexer(name: &str) -> Result<Self, ConfigError> {
        let config = ConfigBuilder::indexer(name).load_with_database_url()?;
        Ok(AppConfig::Indexer(config))
    }

    /// Get database configuration (available in all app types)
    pub fn database(&self) -> &DatabaseConfig {
        match self {
            AppConfig::Api(config) => &config.database,
            AppConfig::Indexer(config) => &config.database,
        }
    }

    /// Get logging configuration (available in all app types)
    pub fn logging(&self) -> &LoggingConfig {
        match self {
            AppConfig::Api(config) => &config.logging,
            AppConfig::Indexer(config) => &config.logging,
        }
    }

    /// Get telemetry configuration (available in all app types)
    pub fn telemetry(&self) -> &TelemetryConfig {
        match self {
            AppConfig::Api(config) => &config.telemetry,
            AppConfig::Indexer(config) => &config.telemetry,
        }
    }

    /// Get Discourse indexing configuration if present (indexer only)
    pub fn discourse(&self) -> Option<&DiscourseConfig> {
        match self {
            AppConfig::Api(_) => None,
            AppConfig::Indexer(config) => config.discourse.as_ref(),
        }
    }

    /// Get server configuration (only available for API)
    pub fn server(&self) -> Option<&ServerConfig> {
        match self {
            AppConfig::Api(config) => Some(&config.server),
            AppConfig::Indexer(_) => None,
        }
    }

    /// Get server configuration (panics if not API)
    pub fn server_required(&self) -> &ServerConfig {
        match self {
            AppConfig::Api(config) => &config.server,
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
        if let Ok(database_url) = std::env::var("DATABASE_URL") {
            config.database.url = database_url;
        }

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
        if let Ok(database_url) = std::env::var("DATABASE_URL") {
            config.database.url = database_url;
        }

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
            worker_threads: None,
        }
    }
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: Some("pretty".to_string()),
        }
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            service_version: "0.1.0".to_string(),
            environment: "development".to_string(),
            enable_metrics: true,
            enable_traces: true,
            enable_logs: true,
            batch_size: 512,
            export_timeout_sec: 10,
            export_interval_sec: 5,
            resource_attributes: std::collections::HashMap::new(),
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
            worker_threads = 4

            [logging]
            level = "info"
            format = "pretty"

            [telemetry]
            enabled = true
            service_version = "0.1.0"
            environment = "development"
            enable_metrics = true
            enable_traces = true
            enable_logs = true
            batch_size = 512
            export_timeout_sec = 10
            export_interval_sec = 5
            resource_attributes = {}
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
            enable_metrics = true
            enable_traces = true
            enable_logs = true
            batch_size = 512
            export_timeout_sec = 10
            export_interval_sec = 5
            resource_attributes = {}
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
                // DATABASE_URL injected
                assert_eq!(cfg.database.url, "mysql://user:pass@localhost/idx");
                // Host config loaded
                assert_eq!(cfg.logging.level, "debug");
                // Env override applied
                assert_eq!(cfg.telemetry.enabled, false);
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
            enable_metrics = true
            enable_traces = true
            enable_logs = true
            batch_size = 512
            export_timeout_sec = 10
            export_interval_sec = 5
            resource_attributes = {}
        "#;
        fs::write(base.join("indexers/indexer-1/config.toml"), idx_cfg).unwrap();

        let old_cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&base).unwrap();
        std::env::set_var("DATABASE_URL", "mysql://user:pass@localhost/panic");

        let app_cfg = AppConfig::load_indexer("indexer-1").unwrap();
        assert!(matches!(app_cfg.server(), None));

        // Cleanup
        std::env::set_current_dir(&old_cwd).unwrap();
        std::env::remove_var("DATABASE_URL");
        let _ = fs::remove_dir_all(&base);
    }
}
