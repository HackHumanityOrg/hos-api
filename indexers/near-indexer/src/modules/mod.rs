pub mod raw;
pub mod venear_governance;

use common::config::{AppConfig, RawModuleConfig, VenearGovernanceModuleConfig};
use std::sync::Arc;

use crate::core::receipt_graph_resolver::ReceiptGraphResolver;
use crate::ModuleRegistry;

const DEFAULT_YIELD_TIMEOUT_BLOCKS: u64 = 200;

pub(crate) fn effective_raw_data_receipt_link_retention_blocks(raw_cfg: &RawModuleConfig) -> i64 {
    i64::try_from(
        raw_cfg
            .data_receipt_link_retention_blocks
            .max(raw_cfg.restart_rewind_blocks),
    )
    .unwrap_or(i64::MAX)
}

pub(crate) fn effective_venear_data_receipt_link_retention_blocks(
    venear_cfg: &VenearGovernanceModuleConfig,
) -> i64 {
    i64::try_from(
        venear_cfg.receipt_tx_map_retention_blocks.max(
            venear_cfg
                .restart_rewind_blocks
                .saturating_add(DEFAULT_YIELD_TIMEOUT_BLOCKS)
                .saturating_add(venear_cfg.finalization_interval_blocks),
        ),
    )
    .unwrap_or(i64::MAX)
}

pub(crate) fn shared_receipt_graph(config: &AppConfig) -> Arc<ReceiptGraphResolver> {
    let mut retention_blocks = 1_i64;

    if let AppConfig::Indexer(indexer) = config {
        if let Some(near) = indexer.near.as_ref() {
            let modules = near.modules.clone().unwrap_or_default();

            if near.raw_enabled_or_default() {
                let raw_cfg = modules.raw.clone().unwrap_or_default();
                retention_blocks = retention_blocks
                    .max(effective_raw_data_receipt_link_retention_blocks(&raw_cfg));
            }

            if let Some(venear_cfg) = modules.venear_governance.as_ref() {
                if venear_cfg.enabled {
                    retention_blocks = retention_blocks.max(
                        effective_venear_data_receipt_link_retention_blocks(venear_cfg),
                    );
                }
            }
        }
    }

    Arc::new(ReceiptGraphResolver::with_data_receipt_link_retention_blocks(retention_blocks))
}

/// Register all available modules
pub fn register_all_modules(registry: &mut ModuleRegistry, config: &common::config::AppConfig) {
    let receipt_graph = shared_receipt_graph(config);

    // Register full raw data module.
    let raw_module = raw::RawModule::with_receipt_graph(config, receipt_graph.clone());
    registry.register_module(Box::new(raw_module));

    // Register HOS-filtered veNEAR governance raw module.
    let venear_governance_module =
        venear_governance::VenearGovernanceModule::with_receipt_graph(config, receipt_graph);
    registry.register_module(Box::new(venear_governance_module));
}
