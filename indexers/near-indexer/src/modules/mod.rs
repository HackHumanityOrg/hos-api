pub mod accounts;
pub mod tokens;

use crate::ModuleRegistry;

/// Register all available modules
pub fn register_all_modules(registry: &mut ModuleRegistry, config: &common::config::AppConfig) {
    // Register accounts module
    let accounts_module = accounts::AccountsModule::new(config);
    registry.register_module(Box::new(accounts_module));

    // Register tokens module
    let tokens_module = tokens::TokensModule::new(config);
    registry.register_module(Box::new(tokens_module));
}
