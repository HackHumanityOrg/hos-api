pub use sea_orm_migration::prelude::*;

mod m20250902_000001_create_discourse_tables;
mod m20250904_000001_near_accounts;
mod m20250904_000002_near_tokens;

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20250902_000001_create_discourse_tables::Migration),
            Box::new(m20250904_000001_near_accounts::Migration),
            Box::new(m20250904_000002_near_tokens::Migration),
        ]
    }
}
