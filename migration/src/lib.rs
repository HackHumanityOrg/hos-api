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
mod m20260304_000012_near_venear_delegation_events;
mod m20260305_000013_near_venear_timeseries_schema;
mod m20260306_000014_near_accounts_timeseries_hardening;

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
        ]
    }
}
