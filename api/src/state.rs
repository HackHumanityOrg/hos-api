//! Application state management

use sea_orm::DatabaseConnection;

/// Application state that can be shared across handlers
#[derive(Clone)]
pub struct AppState {
    /// Optional database connection
    pub db: Option<DatabaseConnection>,
}

impl AppState {
    pub fn new(db: Option<DatabaseConnection>) -> Self {
        Self { db }
    }
}
