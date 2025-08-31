//! Type-safe configuration system with app-specific configurations
//! Supports hierarchical loading from TOML files and environment variables

use crate::errors::ConfigError;
use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};

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

/// Server configuration for API service only
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub worker_threads: Option<usize>,
}

/// Logging configuration per application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: Option<String>,
}

/// API-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub database: DatabaseConfig,
    pub server: ServerConfig,
    pub logging: LoggingConfig,
}

/// Indexer-specific configuration (no server needed)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexerConfig {
    pub database: DatabaseConfig,
    pub logging: LoggingConfig,
}

/// Unified configuration enum for type safety
#[derive(Debug, Clone)]
pub enum AppConfig {
    Api(ApiConfig),
    Indexer(IndexerConfig),
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
        let mut figment = Figment::new().merge(Toml::file("config.toml")); // Workspace config (database only)

        // Add binary-specific config if it exists
        let binary_config = std::path::Path::new(&self.binary_config_path);
        if binary_config.exists() {
            figment = figment.merge(Toml::file(&self.binary_config_path));
        }

        // Environment variables have highest priority
        figment = figment.merge(Env::prefixed("APP_"));

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
            max_connections: Some(10),
            min_connections: Some(1),
            connection_timeout: Some(30),
            acquire_timeout: Some(30),
            idle_timeout: Some(600),  // 10 minutes
            max_lifetime: Some(1800), // 30 minutes
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
