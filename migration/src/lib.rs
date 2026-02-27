pub use sea_orm_migration::prelude::*;

mod m20251001_000001_discourse_schema;
mod m20251001_000002_near_schema;
mod m20260227_000003_discourse_revision_json_columns;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20251001_000001_discourse_schema::Migration),
            Box::new(m20251001_000002_near_schema::Migration),
            Box::new(m20260227_000003_discourse_revision_json_columns::Migration),
        ]
    }
}
