# Database Migrations Guide

This guide covers database schema management using SeaORM migrations in the HOS API platform.

## Overview

The project uses SeaORM's migration system to manage database schema changes in a version-controlled, team-friendly way. All migrations are stored in the `migration/` crate and can be run using convenient cargo aliases.

### Migration Philosophy

- Define schema migrations in Rust code using `SchemaManager` + SeaQuery builders.
- Prefer DSL methods (`create_table`, `alter_table`, `create_index`) over raw SQL.
- Use raw SQL only for operations that are not supported by SeaQuery.
- Keep all migrations reversible with a corresponding `down()` implementation.

## Quick Reference

```bash
# Check what migrations need to be applied
cargo migrate-status

# Apply all pending migrations
cargo migrate-up

# Rollback the last migration
cargo migrate-down

# Reset database (rollback all migrations)
cargo migrate-reset

# Drop all tables from DB, then re-apply all migrations from scratch
cargo migrate-fresh

# Generate a new migration file
cargo migrate-generate create_users_table

# Regenerate entities from the live DB schemas
cargo generate-entities

# Backfill identity account registry from existing source tables
cargo migrate-up
cargo backfill-identity-accounts

# Optional: override NEAR scope used during backfill (default: mainnet)
IDENTITY_NEAR_CHAIN=testnet cargo backfill-identity-accounts
```

## Creating New Migrations

### 1. Generate Migration File

```bash
cargo migrate-generate create_users_table
```

This creates a new file in `migration/src/` with the pattern `m{timestamp}_{name}.rs`.

### 2. Edit Migration File

The generated file contains two methods you need to implement:

```rust
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create table logic here
        manager
            .create_table(
                Table::create()
                    .table(Users::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Users::Id)
                            .integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Users::Email).string().not_null())
                    .col(ColumnDef::new(Users::Name).string().not_null())
                    .col(
                        ColumnDef::new(Users::CreatedAt)
                            .timestamp()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop table logic here
        manager
            .drop_table(Table::drop().table(Users::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Users {
    Table,
    Id,
    Email,
    Name,
    CreatedAt,
}
```

### 3. Register Migration

Add your migration to `migration/src/lib.rs`:

```rust
pub use sea_orm_migration::prelude::*;

mod m20251001_000001_discourse_schema;
mod m20260302_000009_identity_schema; // Add your new migration here

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20251001_000001_discourse_schema::Migration),
            Box::new(m20260302_000009_identity_schema::Migration), // Add your new migration here
        ]
    }
}
```

### 4. Apply Migration

```bash
cargo migrate-up
```

## Migration Patterns

### Creating Tables

```rust
async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
        .create_table(
            Table::create()
                .table(Posts::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(Posts::Id)
                        .integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(ColumnDef::new(Posts::Title).string().not_null())
                .col(ColumnDef::new(Posts::Content).text())
                .col(ColumnDef::new(Posts::UserId).integer().not_null())
                .col(
                    ColumnDef::new(Posts::CreatedAt)
                        .timestamp()
                        .default(Expr::current_timestamp()),
                )
                .foreign_key(
                    ForeignKey::create()
                        .name("fk-posts-user_id")
                        .from(Posts::Table, Posts::UserId)
                        .to(Users::Table, Users::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await
}
```

### Adding Columns

```rust
async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
        .alter_table(
            Table::alter()
                .table(Users::Table)
                .add_column(ColumnDef::new(Users::LastLogin).timestamp())
                .to_owned(),
        )
        .await
}

async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
        .alter_table(
            Table::alter()
                .table(Users::Table)
                .drop_column(Users::LastLogin)
                .to_owned(),
        )
        .await
}
```

### Creating Indexes

```rust
async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
        .create_index(
            Index::create()
                .name("idx-users-email")
                .table(Users::Table)
                .col(Users::Email)
                .unique()
                .to_owned(),
        )
        .await
}

async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager
        .drop_index(Index::drop().name("idx-users-email").to_owned())
        .await
}
```

## Data Migrations

For data-only changes (not schema), you can execute raw SQL:

```rust
async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();
    
    db.execute_unprepared("UPDATE users SET status = 'active' WHERE status IS NULL")
        .await?;
        
    Ok(())
}
```

## Working with Entities

### Generating Entities

After applying migrations, regenerate SeaORM entities from the database:

```bash
cargo generate-entities
```

This command introspects PostgreSQL schemas and updates:

- `shared/entities/src/discourse`
- `shared/entities/src/near`
- `shared/entities/src/telegram`
- `shared/entities/src/identity`

The generator reads `DATABASE_URL` and runs `sea-orm-cli generate entity` under the hood.

### Using Entities in Code

```rust
use entities::{users, prelude::*};
use sea_orm::*;

// Find by ID
let user: Option<users::Model> = Users::find_by_id(1)
    .one(db)
    .await?;

// Create new user
let new_user = users::ActiveModel {
    email: Set("john@example.com".to_owned()),
    name: Set("John Doe".to_owned()),
    ..Default::default()
};

let user: users::Model = new_user.insert(db).await?;
```

## Migration Best Practices

### 1. Always Write Reversible Migrations

Every `up()` should have a corresponding `down()` that undoes the change:

```rust
// Good - reversible
async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager.create_table(/* table definition */).await
}

async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager.drop_table(Table::drop().table(MyTable::Table).to_owned()).await
}
```

### 2. Use Descriptive Names

```bash
# Good
cargo migrate-generate create_users_table
cargo migrate-generate add_email_index_to_users  
cargo migrate-generate remove_deprecated_status_column

# Bad
cargo migrate-generate update_stuff
cargo migrate-generate fix_things
```

### 3. Test Migrations

Before applying to production:

```bash
# Apply migration
cargo migrate-up

# Test your application
cargo test-all

# Test rollback
cargo migrate-down
cargo migrate-up
```

### 4. Handle Large Data Carefully

For large datasets, consider:

```rust
// Process in batches
async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();
    
    loop {
        let result = db.execute_unprepared(
            "UPDATE users SET processed = true WHERE processed = false LIMIT 1000"
        ).await?;
        
        if result.rows_affected() == 0 {
            break;
        }
    }
    
    Ok(())
}
```

## Environment-Specific Migrations

### Development

```bash
# Reset and start fresh frequently
cargo migrate-reset
cargo migrate-up
cargo generate-entities
```

### Production

```bash
# Only apply pending migrations
cargo migrate-status  # Check first
cargo migrate-up      # Apply carefully
```

## Troubleshooting

### Migration Fails

1. **Check syntax**: Ensure your SeaQuery syntax is correct
2. **Check constraints**: Verify foreign keys and constraints exist
3. **Check data**: Ensure existing data is compatible

### Rollback Issues

1. **Check dependencies**: Other tables might reference your data
2. **Manual cleanup**: You might need to clean up data first
3. **Force reset**: Use `cargo migrate-reset` for development

### Entity Generation Issues

```bash
# Ensure schema is current and regenerate from DB
cargo migrate-status
cargo generate-entities
```

## Configuration

### Database Connection

Migration commands use the same database configuration as your application:

```bash
# Set in .env
DATABASE_URL=postgresql://user:pass@host:5432/database?sslmode=require

# Or in config.toml
[database]
max_connections = 10
connection_timeout = 30
```

If you use Doppler + direnv, ensure env is loaded first:

```bash
direnv allow
```

### Custom Migration Path

```bash
# Run migrations from custom directory
sea-orm-cli migrate up -d ./custom/migration/path
```

## Advanced Usage

### Conditional Migrations

`SchemaManager` provides inspection methods to guard idempotent migrations:

- `manager.has_table("table_name").await?` → `Result<bool, DbErr>`
- `manager.has_column("table_name", "column_name").await?` → `Result<bool, DbErr>`
- `manager.has_index("table_name", "index_name").await?` → `Result<bool, DbErr>`

```rust
async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    // Check if column already exists before adding it
    if !manager.has_column("users", "email").await? {
        manager
            .alter_table(
                Table::alter()
                    .table(Users::Table)
                    .add_column(ColumnDef::new(Users::Email).string())
                    .to_owned(),
            )
            .await?;
    }

    // Check if index already exists before creating it
    if !manager.has_index("users", "idx-users-email").await? {
        manager
            .create_index(
                Index::create()
                    .name("idx-users-email")
                    .table(Users::Table)
                    .col(Users::Email)
                    .to_owned(),
            )
            .await?;
    }

    Ok(())
}
```

### Using Transactions

Each migration's `up()` and `down()` method is automatically wrapped in a database transaction by the SeaORM migration framework. If an error is returned, the entire migration is rolled back.

If you need a savepoint within a migration (to isolate a subset of operations that may fail), you can use `begin()` on the connection — in PostgreSQL this creates a nested savepoint, not a new top-level transaction:

```rust
async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let conn = manager.get_connection();

    // Creates a savepoint (nested transaction) within the migration's outer transaction
    let txn = conn.begin().await?;
    // Operations that should be isolated
    txn.commit().await?;

    Ok(())
}
```

---

For more advanced patterns and examples, check the [SeaORM Migration Documentation](https://www.sea-ql.org/SeaORM/docs/migration/setting-up-migration/).
