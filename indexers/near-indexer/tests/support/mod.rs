#![allow(dead_code)]

pub mod fixture_catalog;
pub mod fixture_expectations;
pub mod fixture_ranges;
pub mod hos_mv_goldens;

pub use test_support::{init_test_tracing, TestDatabase};

pub fn skip_resource_heavy_test_in_ci(test_name: &str) -> bool {
    let should_skip = std::env::var_os("SKIP_RESOURCE_HEAVY_TESTS").is_some();
    if should_skip {
        eprintln!(
            "skipping resource-heavy test {test_name} because SKIP_RESOURCE_HEAVY_TESTS is set"
        );
    }
    should_skip
}
