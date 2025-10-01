//! Application state management

use sea_orm::DatabaseConnection;
use std::sync::Arc;

/// Application state that can be shared across handlers
#[derive(Clone)]
pub struct AppState {
    /// Optional database connection wrapped in Arc for cheap cloning
    pub db: Option<Arc<DatabaseConnection>>,
}

impl AppState {
    /// Create a new application state from an optional shared database connection
    pub fn new(db: Option<Arc<DatabaseConnection>>) -> Self {
        Self { db }
    }

    /// Helper to wrap an owned database connection into the shared state
    pub fn from_database(db: Option<DatabaseConnection>) -> Self {
        Self::new(db.map(Arc::new))
    }
}
