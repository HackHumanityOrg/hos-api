pub use sea_orm_migration::prelude::*;

mod m20251001_000001_discourse_schema;
mod m20251001_000002_near_schema;
mod m20260227_000003_discourse_revision_json_columns;
mod m20260227_000004_discourse_fk_constraints;
mod m20260227_000005_near_venear_governance_schema;
mod m20260228_000006_json_to_jsonb;
mod m20260301_000007_telegram_schema;
mod m20260301_000008_drop_telegram_import_logs;
mod m20260302_000009_identity_schema;
mod m20260303_000010_identity_person_name;
mod m20260303_000011_identity_venear_platforms;
mod m20260303_000015_near_raw_schema_improvements;
mod m20260304_000012_near_venear_delegation_events;
mod m20260305_000013_near_venear_timeseries_schema;
mod m20260306_000014_near_accounts_timeseries_hardening;
mod m20260307_000016_near_venear_coverage_expansion;
mod m20260308_000017_near_venear_transaction_hashes;
mod m20260309_000018_near_venear_raw_enrichment;
mod m20260310_000019_near_hos_raw_refactor;
mod m20260310_000020_near_relevant_receipts_generalization;
mod m20260310_000021_near_hos_sql_static_config;
mod m20260311_000022_near_hos_mv_parity_fixes;
mod m20260311_000023_drop_hos_approved_proposals;
mod m20260312_000024_restore_venear_receipt_tx_origins;
mod m20260312_000025_rename_near_relevant_receipts_table;
mod m20260313_000026_hos_whole_tx_finalization;
mod m20260313_000027_near_receipt_tx_hash_map;
mod m20260313_000028_hos_matcher_and_raw_parity;
mod m20260313_000029_in_memory_hos_staging_refactor;
mod m20260313_000030_near_execution_outcomes_exact_fee_fields;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20251001_000001_discourse_schema::Migration),
            Box::new(m20251001_000002_near_schema::Migration),
            Box::new(m20260227_000003_discourse_revision_json_columns::Migration),
            Box::new(m20260227_000004_discourse_fk_constraints::Migration),
            Box::new(m20260227_000005_near_venear_governance_schema::Migration),
            Box::new(m20260228_000006_json_to_jsonb::Migration),
            Box::new(m20260301_000007_telegram_schema::Migration),
            Box::new(m20260301_000008_drop_telegram_import_logs::Migration),
            Box::new(m20260302_000009_identity_schema::Migration),
            Box::new(m20260303_000010_identity_person_name::Migration),
            Box::new(m20260303_000011_identity_venear_platforms::Migration),
            Box::new(m20260304_000012_near_venear_delegation_events::Migration),
            Box::new(m20260305_000013_near_venear_timeseries_schema::Migration),
            Box::new(m20260306_000014_near_accounts_timeseries_hardening::Migration),
            Box::new(m20260303_000015_near_raw_schema_improvements::Migration),
            Box::new(m20260307_000016_near_venear_coverage_expansion::Migration),
            Box::new(m20260308_000017_near_venear_transaction_hashes::Migration),
            Box::new(m20260309_000018_near_venear_raw_enrichment::Migration),
            Box::new(m20260310_000019_near_hos_raw_refactor::Migration),
            Box::new(m20260310_000020_near_relevant_receipts_generalization::Migration),
            Box::new(m20260310_000021_near_hos_sql_static_config::Migration),
            Box::new(m20260311_000022_near_hos_mv_parity_fixes::Migration),
            Box::new(m20260311_000023_drop_hos_approved_proposals::Migration),
            Box::new(m20260312_000024_restore_venear_receipt_tx_origins::Migration),
            Box::new(m20260312_000025_rename_near_relevant_receipts_table::Migration),
            Box::new(m20260313_000026_hos_whole_tx_finalization::Migration),
            Box::new(m20260313_000027_near_receipt_tx_hash_map::Migration),
            Box::new(m20260313_000028_hos_matcher_and_raw_parity::Migration),
            Box::new(m20260313_000029_in_memory_hos_staging_refactor::Migration),
            Box::new(m20260313_000030_near_execution_outcomes_exact_fee_fields::Migration),
        ]
    }
}
