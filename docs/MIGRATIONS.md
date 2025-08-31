# Database Migrations Guide

This guide covers database schema management using SeaORM migrations in the Data Commons platform.

## Overview

The project uses SeaORM's migration system to manage database schema changes in a version-controlled, team-friendly way. All migrations are stored in the `migration/` crate and can be run using convenient cargo aliases.

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

# Fresh start (reset + migrate up)
cargo migrate-fresh

# Generate a new migration file
cargo migrate-generate create_users_table
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

mod m20220101_000001_create_table;
mod m20240831_120000_create_users_table; // Add this line

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20220101_000001_create_table::Migration),
            Box::new(m20240831_120000_create_users_table::Migration), // Add this line
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

After applying migrations, regenerate SeaORM entities:

```bash
cargo generate-entities
```

This updates files in `shared/entities/src/` to match your current database schema.

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
// ✅ Good - reversible
async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager.create_table(/* table definition */).await
}

async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    manager.drop_table(Table::drop().table(MyTable::Table).to_owned()).await
}
```

### 2. Use Descriptive Names

```bash
# ✅ Good
cargo migrate-generate create_users_table
cargo migrate-generate add_email_index_to_users  
cargo migrate-generate remove_deprecated_status_column

# ❌ Bad
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
# If entities aren't updating
rm -rf shared/entities/src/*.rs
cargo generate-entities
```

## Configuration

### Database Connection

Migration commands use the same database configuration as your application:

```bash
# Set in .env
DATABASE_URL=mysql://user:pass@host/database

# Or in config.toml
[database]
max_connections = 10
connection_timeout = 30
```

### Custom Migration Path

```bash
# Run migrations from custom directory
sea-orm-cli migrate up -d ./custom/migration/path
```

## Advanced Usage

### Conditional Migrations

```rust
async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let db = manager.get_connection();
    
    // Check if column already exists
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
    
    Ok(())
}
```

### Using Transactions

Migrations automatically run in transactions, but you can create subtransactions:

```rust
async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
    let txn = manager.get_connection().begin().await?;
    
    // Multiple operations here
    
    txn.commit().await?;
    Ok(())
}
```

---

For more advanced patterns and examples, check the [SeaORM Migration Documentation](https://www.sea-ql.org/SeaORM/docs/migration/setting-up-migration/).