pub mod accounts;
pub mod raw;
pub mod tokens;

use crate::ModuleRegistry;

/// Register all available modules
pub fn register_all_modules(registry: &mut ModuleRegistry, config: &common::config::AppConfig) {
    // Register raw data module - stores all NEAR blockchain data as-is
    let raw_module = raw::RawModule::new(config);
    registry.register_module(Box::new(raw_module));

    // Register accounts module - tracks account lifecycle and metadata
    let accounts_module = accounts::AccountsModule::new(config);
    registry.register_module(Box::new(accounts_module));

    // Register tokens module - indexes NEP-141, NEP-171, and NEP-245 token events
    let tokens_module = tokens::TokensModule::new(config);
    registry.register_module(Box::new(tokens_module));
}
