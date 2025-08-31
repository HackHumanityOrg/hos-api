//! Database connection management utilities
//!
//! This crate provides database connection management for PlanetScale MySQL
//! with SSL/TLS support. Use sea-orm-cli for migrations and SeaORM's built-in
//! transaction methods directly.

pub mod connection;

// Re-export commonly used types
pub use connection::*;
