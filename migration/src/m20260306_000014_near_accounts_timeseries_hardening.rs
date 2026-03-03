use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let statements: &[&str] = &[
            // Snapshot hardening for near_accounts.
            "ALTER TABLE near.near_accounts ADD COLUMN IF NOT EXISTS last_seen_height BIGINT NOT NULL DEFAULT 0",
            "ALTER TABLE near.near_accounts ADD COLUMN IF NOT EXISTS last_seen_timestamp BIGINT NOT NULL DEFAULT 0",
            "ALTER TABLE near.near_accounts ADD COLUMN IF NOT EXISTS global_contract_account_id VARCHAR(128)",
            "ALTER TABLE near.near_accounts ADD COLUMN IF NOT EXISTS global_contract_hash VARCHAR(128)",
            "UPDATE near.near_accounts
             SET last_seen_height = created_at_height,
                 last_seen_timestamp = created_at_timestamp
             WHERE last_seen_height = 0",
            // Replay-safe identity and query indexes for balances.
            "ALTER TABLE near.near_account_balances ADD COLUMN IF NOT EXISTS shard_id INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE near.near_account_balances ADD COLUMN IF NOT EXISTS state_change_index INTEGER NOT NULL DEFAULT 0",
            "WITH ranked AS (
                 SELECT id,
                        (ROW_NUMBER() OVER (PARTITION BY block_height, account_id ORDER BY id) - 1)::INTEGER AS rn
                 FROM near.near_account_balances
             )
             UPDATE near.near_account_balances AS b
             SET state_change_index = ranked.rn
             FROM ranked
             WHERE b.id = ranked.id",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_near_account_balances_identity
             ON near.near_account_balances (block_height, shard_id, state_change_index, account_id)",
            "CREATE INDEX IF NOT EXISTS ix_near_account_balances_account_height
             ON near.near_account_balances (account_id, block_height)",
            // Replay-safe identity and query indexes for events.
            "ALTER TABLE near.near_account_events ADD COLUMN IF NOT EXISTS shard_id INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE near.near_account_events ADD COLUMN IF NOT EXISTS state_change_index INTEGER NOT NULL DEFAULT 0",
            "ALTER TABLE near.near_account_events ADD COLUMN IF NOT EXISTS event_index INTEGER NOT NULL DEFAULT 0",
            "WITH ranked AS (
                 SELECT id,
                        (ROW_NUMBER() OVER (PARTITION BY block_height, account_id ORDER BY id) - 1)::INTEGER AS rn
                 FROM near.near_account_events
             )
             UPDATE near.near_account_events AS e
             SET state_change_index = 0,
                 event_index = ranked.rn
             FROM ranked
             WHERE e.id = ranked.id",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_near_account_events_identity
             ON near.near_account_events (block_height, shard_id, state_change_index, event_index, account_id)",
            "CREATE INDEX IF NOT EXISTS ix_near_account_events_account_height
             ON near.near_account_events (account_id, block_height)",
            "CREATE INDEX IF NOT EXISTS ix_near_account_events_type_height
             ON near.near_account_events (event_type, block_height)",
            // Access key replay/out-of-order hardening.
            "ALTER TABLE near.near_access_keys ADD COLUMN IF NOT EXISTS last_seen_height BIGINT NOT NULL DEFAULT 0",
            "UPDATE near.near_access_keys
             SET last_seen_height = COALESCE(deleted_at_height, created_at_height)
             WHERE last_seen_height = 0",
            "DELETE FROM near.near_access_keys AS a
             USING near.near_access_keys AS b
             WHERE a.id > b.id
               AND a.account_id = b.account_id
               AND a.public_key = b.public_key
               AND a.created_at_height = b.created_at_height",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_near_access_keys_lifecycle_start
             ON near.near_access_keys (account_id, public_key, created_at_height)",
            "WITH ranked AS (
                 SELECT id,
                        ROW_NUMBER() OVER (
                            PARTITION BY account_id, public_key
                            ORDER BY last_seen_height DESC, created_at_height DESC, id DESC
                        ) AS rn
                 FROM near.near_access_keys
                 WHERE is_active = true
             )
             UPDATE near.near_access_keys AS k
             SET is_active = false,
                 deleted_at_height = COALESCE(k.deleted_at_height, k.last_seen_height)
             FROM ranked
             WHERE k.id = ranked.id
               AND ranked.rn > 1",
            "CREATE UNIQUE INDEX IF NOT EXISTS ux_near_access_keys_active
             ON near.near_access_keys (account_id, public_key)
             WHERE is_active = true",
            "CREATE INDEX IF NOT EXISTS ix_near_access_keys_account_active
             ON near.near_access_keys (account_id, is_active)",
        ];

        for sql in statements {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let statements: &[&str] = &[
            "DROP INDEX IF EXISTS near.ix_near_access_keys_account_active",
            "DROP INDEX IF EXISTS near.ux_near_access_keys_active",
            "DROP INDEX IF EXISTS near.ux_near_access_keys_lifecycle_start",
            "ALTER TABLE near.near_access_keys DROP COLUMN IF EXISTS last_seen_height",
            "DROP INDEX IF EXISTS near.ix_near_account_events_type_height",
            "DROP INDEX IF EXISTS near.ix_near_account_events_account_height",
            "DROP INDEX IF EXISTS near.ux_near_account_events_identity",
            "ALTER TABLE near.near_account_events DROP COLUMN IF EXISTS event_index",
            "ALTER TABLE near.near_account_events DROP COLUMN IF EXISTS state_change_index",
            "ALTER TABLE near.near_account_events DROP COLUMN IF EXISTS shard_id",
            "DROP INDEX IF EXISTS near.ix_near_account_balances_account_height",
            "DROP INDEX IF EXISTS near.ux_near_account_balances_identity",
            "ALTER TABLE near.near_account_balances DROP COLUMN IF EXISTS state_change_index",
            "ALTER TABLE near.near_account_balances DROP COLUMN IF EXISTS shard_id",
            "ALTER TABLE near.near_accounts DROP COLUMN IF EXISTS global_contract_hash",
            "ALTER TABLE near.near_accounts DROP COLUMN IF EXISTS global_contract_account_id",
            "ALTER TABLE near.near_accounts DROP COLUMN IF EXISTS last_seen_timestamp",
            "ALTER TABLE near.near_accounts DROP COLUMN IF EXISTS last_seen_height",
        ];

        for sql in statements {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }
}
