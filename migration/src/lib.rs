pub use sea_orm_migration::prelude::*;

mod m20250902_000001_create_discourse_tables;
mod m20250904_000002_near_comprehensive;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250902_000001_create_discourse_tables::Migration),
            Box::new(m20250904_000002_near_comprehensive::Migration),
        ]
    }
}
