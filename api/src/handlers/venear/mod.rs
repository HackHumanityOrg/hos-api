//! Canonical-query veNEAR governance handlers.
#![allow(clippy::result_large_err)]

pub mod accounts;
pub mod activity;
pub mod admin;
pub mod config;
pub mod delegations;
pub mod lockups;
pub mod proposals;
pub mod sql;

pub use accounts::*;
pub use activity::*;
pub use admin::*;
pub use config::*;
pub use delegations::*;
pub use lockups::*;
pub use proposals::*;
