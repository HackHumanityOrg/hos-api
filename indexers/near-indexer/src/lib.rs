use anyhow::Result;
use async_trait::async_trait;
use sea_orm::DatabaseConnection;
use tracing::{debug, info};

pub mod core;
pub mod modules;

/// Core trait that all indexer modules must implement
#[async_trait]
pub trait IndexerModule: Send + Sync {
    /// Unique name for this module (used for cursor tracking and logging)
    fn name(&self) -> &str;

    /// Whether this module is enabled and should process blocks
    fn enabled(&self) -> bool;

    /// Stream assignment mode for this module.
    /// Defaults to shared stream execution.
    fn stream_mode(&self) -> common::config::NearModuleStreamMode {
        common::config::NearModuleStreamMode::Shared
    }

    /// Process a single streamer message for this module
    async fn process_block(
        &self,
        msg: &near_lake_framework::near_indexer_primitives::StreamerMessage,
        db: &DatabaseConnection,
    ) -> Result<()>;

    /// Get the last processed block height for this module
    async fn get_cursor(&self, db: &DatabaseConnection) -> Result<Option<i64>>;

    /// Update the cursor for this module after successful processing
    async fn update_cursor(&self, db: &DatabaseConnection, height: i64) -> Result<()>;

    /// Initialize any module-specific setup (tables, indices, etc.)
    async fn initialize(&self, _db: &DatabaseConnection) -> Result<()> {
        // Default implementation does nothing
        Ok(())
    }
}

/// Registry that manages all indexer modules
pub struct ModuleRegistry {
    modules: Vec<Box<dyn IndexerModule>>,
}

impl ModuleRegistry {
    pub fn new() -> Self {
        Self {
            modules: Vec::new(),
        }
    }

    pub fn register_module(&mut self, module: Box<dyn IndexerModule>) {
        let module_name = module.name().to_string();
        let enabled = module.enabled();
        let stream_mode = module.stream_mode();
        self.modules.push(module);

        debug!(
            module = %module_name,
            enabled = enabled,
            stream_mode = stream_mode.as_str(),
            total_modules = self.modules.len(),
            "Registered indexer module"
        );
    }

    pub fn enabled_modules(&self) -> impl Iterator<Item = &dyn IndexerModule> {
        self.modules
            .iter()
            .filter(|m| m.enabled())
            .map(|m| m.as_ref())
    }

    pub fn enabled_module_names(&self) -> Vec<&str> {
        self.enabled_modules().map(|m| m.name()).collect()
    }

    pub fn all_modules(&self) -> impl Iterator<Item = &dyn IndexerModule> {
        self.modules.iter().map(|m| m.as_ref())
    }

    /// Initialize all registered modules
    pub async fn initialize_all(&self, db: &DatabaseConnection) -> Result<()> {
        info!("Initializing all registered indexer modules");

        let enabled_modules: Vec<_> = self.enabled_modules().collect();
        info!(
            total_modules = self.modules.len(),
            enabled_modules = enabled_modules.len(),
            "Module registry summary"
        );

        for module in &self.modules {
            debug!(
                module = module.name(),
                enabled = module.enabled(),
                "Initializing module"
            );
            module.initialize(db).await?;
        }

        info!("All indexer modules initialized successfully");
        Ok(())
    }
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self::new()
    }
}
