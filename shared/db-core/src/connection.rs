//! Database connection management for PlanetScale Postgres with SSL/TLS

use common::config::DatabaseConfig;
use common::errors::{ConfigError, DatabaseError};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};
use std::time::Duration;
use tracing::info;

/// Establish a connection to the database with PlanetScale-compatible SSL settings
pub async fn establish_connection(
    config: &DatabaseConfig,
) -> Result<DatabaseConnection, DatabaseError> {
    info!("Establishing database connection...");

    let mut opt = ConnectOptions::new(&config.url);

    // Configure connection pool settings from config
    opt.max_connections(config.max_connections.unwrap_or(10))
        .min_connections(config.min_connections.unwrap_or(1))
        .connect_timeout(Duration::from_secs(config.connection_timeout.unwrap_or(30)))
        .acquire_timeout(Duration::from_secs(config.acquire_timeout.unwrap_or(30)))
        .idle_timeout(Duration::from_secs(config.idle_timeout.unwrap_or(600)))
        .max_lifetime(Duration::from_secs(config.max_lifetime.unwrap_or(1800)))
        // Configure SQLx logging to provide meaningful database query messages
        .sqlx_logging(true)
        .sqlx_logging_level(tracing::log::LevelFilter::Warn);

    let db = Database::connect(opt)
        .await
        .map_err(|e| DatabaseError::ConnectionFailed {
            message: e.to_string(),
        })?;

    info!("Database connection established successfully with max_connections={}, min_connections={}, connection_timeout={}s, acquire_timeout={}s, idle_timeout={}s, max_lifetime={}s",
          config.max_connections.unwrap_or(10),
          config.min_connections.unwrap_or(1),
          config.connection_timeout.unwrap_or(30),
          config.acquire_timeout.unwrap_or(30),
          config.idle_timeout.unwrap_or(600),
          config.max_lifetime.unwrap_or(1800));
    Ok(db)
}

/// Create a database connection using configuration
pub async fn from_config(config: &DatabaseConfig) -> Result<DatabaseConnection, DatabaseError> {
    establish_connection(config).await
}

/// Create a database connection from environment variables (legacy)
/// This loads a minimal config with just the DATABASE_URL
pub async fn from_env() -> Result<DatabaseConnection, ConfigError> {
    let database_url = std::env::var("DATABASE_URL").map_err(|_| ConfigError::MissingEnvVar {
        var_name: "DATABASE_URL".to_string(),
    })?;

    // Create a minimal config with DATABASE_URL and default values
    let config = DatabaseConfig {
        url: database_url,
        max_connections: Some(10),
        min_connections: Some(1),
        connection_timeout: Some(30),
        acquire_timeout: Some(30),
        idle_timeout: Some(600),
        max_lifetime: Some(1800),
    };

    establish_connection(&config)
        .await
        .map_err(|_e| ConfigError::InvalidValue {
            key: "DATABASE_URL".to_string(),
            value: "invalid connection string".to_string(),
        })
}
