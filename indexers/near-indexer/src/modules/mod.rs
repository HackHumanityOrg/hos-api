pub mod raw;
pub mod venear_governance;

use crate::ModuleRegistry;

/// Register all available modules
pub fn register_all_modules(registry: &mut ModuleRegistry, config: &common::config::AppConfig) {
    // Register full raw data module.
    let raw_module = raw::RawModule::new(config);
    registry.register_module(Box::new(raw_module));

    // Register HOS-filtered veNEAR governance raw module.
    let venear_governance_module = venear_governance::VenearGovernanceModule::new(config);
    registry.register_module(Box::new(venear_governance_module));
}
