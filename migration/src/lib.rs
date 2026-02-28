pub use sea_orm_migration::prelude::*;

mod m20251001_000001_discourse_schema;
mod m20251001_000002_near_schema;
mod m20260227_000003_discourse_revision_json_columns;
mod m20260227_000004_discourse_fk_constraints;
mod m20260227_000005_near_venear_governance_schema;
mod m20260228_000006_json_to_jsonb;

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
        ]
    }
}
