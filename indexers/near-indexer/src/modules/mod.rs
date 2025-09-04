pub mod accounts;
pub mod tokens;
pub mod transactions;

use crate::ModuleRegistry;

/// Register all available modules
pub fn register_all_modules(registry: &mut ModuleRegistry, config: &common::config::AppConfig) {
    // Register transactions module first (needed by other modules for foreign keys)
    let transactions_enabled = match config {
        common::config::AppConfig::Indexer(indexer_config) => {
            indexer_config
                .near
                .as_ref()
                .and_then(|near_config| near_config.modules.as_ref())
                .map(|_modules| true) // For now, always enable if modules config exists
                .unwrap_or(true)
        }
        _ => true,
    };
    let transactions_module = transactions::TransactionsProcessor::new(transactions_enabled);
    registry.register_module(Box::new(transactions_module));

    // Register accounts module
    let accounts_module = accounts::AccountsModule::new(config);
    registry.register_module(Box::new(accounts_module));

    // Register tokens module
    let tokens_module = tokens::TokensModule::new(config);
    registry.register_module(Box::new(tokens_module));
}
