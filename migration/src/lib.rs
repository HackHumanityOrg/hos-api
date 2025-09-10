pub use sea_orm_migration::prelude::*;

mod m20250902_000001_create_discourse_tables;
mod m20250904_000002_near_raw_data;
mod m20250909_000004_near_account_tables;
mod m20250910_000001_near_token_tables;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250902_000001_create_discourse_tables::Migration),
            Box::new(m20250904_000002_near_raw_data::Migration),
            Box::new(m20250909_000004_near_account_tables::Migration),
            Box::new(m20250910_000001_near_token_tables::Migration),
        ]
    }
}
