pub mod raw;
pub mod venear_governance;

use std::sync::Arc;

use crate::core::receipt_graph_resolver::ReceiptGraphResolver;
use crate::ModuleRegistry;

/// Register all available modules
pub fn register_all_modules(registry: &mut ModuleRegistry, config: &common::config::AppConfig) {
    let receipt_graph = Arc::new(ReceiptGraphResolver::new());

    // Register full raw data module.
    let raw_module = raw::RawModule::with_receipt_graph(config, receipt_graph.clone());
    registry.register_module(Box::new(raw_module));

    // Register HOS-filtered veNEAR governance raw module.
    let venear_governance_module =
        venear_governance::VenearGovernanceModule::with_receipt_graph(config, receipt_graph);
    registry.register_module(Box::new(venear_governance_module));
}
