//! Configuration utilities using Figment for hierarchical configuration
//! Supports loading from TOML files, environment variables, and more

use crate::errors::ConfigError;
use figment::{
    providers::{Env, Format, Toml},
    Figment,
};
use serde::{Deserialize, Serialize};

/// Database configuration structure
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

/// Server configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub worker_threads: Option<usize>,
}

/// Logging configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub format: Option<String>,
}

/// Main application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[derive(Default)]
pub struct AppConfig {
    pub database: DatabaseConfig,
    pub server: ServerConfig,
    pub logging: LoggingConfig,
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


impl AppConfig {
    /// Load configuration from multiple sources in order of precedence:
    /// 1. Environment variables (highest priority)
    /// 2. config.toml file
    /// 3. Default values (lowest priority)
    pub fn load() -> Result<Self, ConfigError> {
        let mut config: AppConfig = Figment::new()
            .merge(Toml::file("config.toml"))
            .merge(Env::prefixed("APP_"))
            .extract()
            .map_err(|e| ConfigError::ParseError {
                message: e.to_string(),
            })?;

        // Load DATABASE_URL from environment
        if let Ok(database_url) = std::env::var("DATABASE_URL") {
            config.database.url = database_url;
        }

        Ok(config)
    }

    /// Load configuration with a custom TOML file path
    pub fn load_from_file<P: AsRef<std::path::Path>>(path: P) -> Result<Self, ConfigError> {
        if !path.as_ref().exists() {
            return Err(ConfigError::FileNotFound {
                path: path.as_ref().display().to_string(),
            });
        }

        let mut config: AppConfig = Figment::new()
            .merge(Toml::file(path))
            .merge(Env::prefixed("APP_"))
            .extract()
            .map_err(|e| ConfigError::ParseError {
                message: e.to_string(),
            })?;

        // Load DATABASE_URL from environment
        if let Ok(database_url) = std::env::var("DATABASE_URL") {
            config.database.url = database_url;
        }

        Ok(config)
    }
}
