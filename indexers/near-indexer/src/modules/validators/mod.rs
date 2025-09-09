pub mod processor;
pub mod rpc_updater;

pub use processor::ValidatorsModule;
pub use rpc_updater::{HistoricalQuery, StakingPoolRpcUpdater};
