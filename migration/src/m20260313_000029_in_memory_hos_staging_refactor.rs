use sea_orm_migration::prelude::*;

const DROP_HOS_STAGING_SCHEMA: &[&str] = &[
    "DROP INDEX IF EXISTS near.ix_near_hos_staging_state_changes_tx_hash",
    "DROP INDEX IF EXISTS near.ix_near_hos_staging_chunks_tx_hash",
    "DROP INDEX IF EXISTS near.ix_near_hos_staging_blocks_height",
    "DROP INDEX IF EXISTS near.ix_near_hos_staging_receipt_status_tx_pending",
    "DROP INDEX IF EXISTS near.ix_near_hos_staging_receipt_status_tx_hash",
    "DROP INDEX IF EXISTS near.ix_near_hos_staging_receipt_edges_tx_hash",
    "DROP INDEX IF EXISTS near.ix_near_hos_staging_receipt_actions_tx_hash",
    "DROP INDEX IF EXISTS near.ix_near_hos_staging_execution_outcomes_tx_hash",
    "DROP INDEX IF EXISTS near.ix_near_hos_staging_receipts_tx_hash",
    "DROP INDEX IF EXISTS near.ix_near_hos_staging_transactions_last_seen",
    "DROP INDEX IF EXISTS near.ix_near_hos_tx_state_touch_status",
    "DROP INDEX IF EXISTS near.ix_near_hos_tx_state_status_last_seen",
    "DROP TABLE IF EXISTS near.near_hos_staging_state_changes",
    "DROP TABLE IF EXISTS near.near_hos_staging_chunks",
    "DROP TABLE IF EXISTS near.near_hos_staging_blocks",
    "DROP TABLE IF EXISTS near.near_hos_staging_receipt_status",
    "DROP TABLE IF EXISTS near.near_hos_staging_receipt_edges",
    "DROP TABLE IF EXISTS near.near_hos_staging_receipt_actions",
    "DROP TABLE IF EXISTS near.near_hos_staging_execution_outcomes",
    "DROP TABLE IF EXISTS near.near_hos_staging_receipts",
    "DROP TABLE IF EXISTS near.near_hos_staging_transactions",
    "DROP TABLE IF EXISTS near.near_hos_tx_state",
];

// Restore the HoS staging schema from m20260313_000026 and m20260313_000028.
const RESTORE_HOS_STAGING_SCHEMA: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS near.near_hos_tx_state (\
        graph_key TEXT PRIMARY KEY,\
        transaction_hash TEXT UNIQUE,\
        status TEXT NOT NULL DEFAULT 'pending',\
        pending_receipt_count BIGINT NOT NULL DEFAULT 0,\
        has_hos_touch BOOLEAN NOT NULL DEFAULT FALSE,\
        first_seen_height BIGINT NOT NULL,\
        last_seen_height BIGINT NOT NULL,\
        finalized_at TIMESTAMPTZ NULL,\
        representative_receipt_id TEXT NULL,\
        incomplete BOOLEAN NOT NULL DEFAULT FALSE,\
        error_reason TEXT NULL,\
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),\
        updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()\
    )",
    "CREATE INDEX IF NOT EXISTS ix_near_hos_tx_state_status_last_seen \
       ON near.near_hos_tx_state (status, last_seen_height)",
    "CREATE INDEX IF NOT EXISTS ix_near_hos_tx_state_touch_status \
       ON near.near_hos_tx_state (has_hos_touch, status)",
    "CREATE TABLE IF NOT EXISTS near.near_hos_staging_transactions (\
        hash TEXT PRIMARY KEY,\
        block_height BIGINT NOT NULL,\
        signer_id TEXT NOT NULL,\
        receiver_id TEXT NOT NULL,\
        nonce BIGINT NOT NULL,\
        priority_fee BIGINT NOT NULL,\
        public_key TEXT NOT NULL,\
        signature TEXT NOT NULL,\
        actions JSONB NOT NULL,\
        raw_data JSONB NULL,\
        shard_id BIGINT NOT NULL,\
        first_seen_height BIGINT NOT NULL,\
        last_seen_height BIGINT NOT NULL\
    )",
    "CREATE INDEX IF NOT EXISTS ix_near_hos_staging_transactions_last_seen \
       ON near.near_hos_staging_transactions (last_seen_height)",
    "CREATE TABLE IF NOT EXISTS near.near_hos_staging_receipts (\
        receipt_id TEXT PRIMARY KEY,\
        transaction_hash TEXT NOT NULL,\
        block_height BIGINT NOT NULL,\
        shard_id BIGINT NOT NULL,\
        predecessor_id TEXT NOT NULL,\
        receiver_id TEXT NOT NULL,\
        receipt JSONB NOT NULL,\
        raw_data JSONB NULL,\
        first_seen_height BIGINT NOT NULL,\
        last_seen_height BIGINT NOT NULL\
    )",
    "CREATE INDEX IF NOT EXISTS ix_near_hos_staging_receipts_tx_hash \
       ON near.near_hos_staging_receipts (transaction_hash)",
    "CREATE TABLE IF NOT EXISTS near.near_hos_staging_execution_outcomes (\
        id TEXT PRIMARY KEY,\
        transaction_hash TEXT NOT NULL,\
        block_height BIGINT NOT NULL,\
        shard_id BIGINT NOT NULL,\
        receipt_id TEXT NULL,\
        outcome JSONB NOT NULL,\
        raw_data JSONB NULL,\
        block_hash TEXT NULL,\
        chunk_hash TEXT NULL,\
        gas_burnt BIGINT NULL,\
        gas_used DOUBLE PRECISION NULL,\
        tokens_burnt DOUBLE PRECISION NULL,\
        executor_account_id TEXT NULL,\
        status TEXT NULL,\
        outcome_receipt_ids JSONB NULL,\
        executed_in_block_hash TEXT NULL,\
        logs JSONB NULL,\
        results_json JSONB NULL,\
        block_timestamp BIGINT NULL,\
        first_seen_height BIGINT NOT NULL,\
        last_seen_height BIGINT NOT NULL\
    )",
    "CREATE INDEX IF NOT EXISTS ix_near_hos_staging_execution_outcomes_tx_hash \
       ON near.near_hos_staging_execution_outcomes (transaction_hash)",
    "CREATE TABLE IF NOT EXISTS near.near_hos_staging_receipt_actions (\
        id TEXT PRIMARY KEY,\
        transaction_hash TEXT NOT NULL,\
        block_height BIGINT NOT NULL,\
        receipt_id TEXT NOT NULL,\
        execution_outcome_id TEXT NOT NULL,\
        shard_id BIGINT NOT NULL,\
        signer_account_id TEXT NULL,\
        signer_public_key TEXT NULL,\
        gas_price TEXT NULL,\
        action_kind TEXT NOT NULL,\
        action_json JSONB NOT NULL,\
        predecessor_id TEXT NOT NULL,\
        receiver_id TEXT NOT NULL,\
        block_hash TEXT NOT NULL,\
        chunk_hash TEXT NOT NULL,\
        author TEXT NOT NULL,\
        method_name TEXT NULL,\
        gas BIGINT NULL,\
        deposit TEXT NULL,\
        args_base64 TEXT NULL,\
        args_json JSONB NULL,\
        action_index INTEGER NOT NULL,\
        block_timestamp BIGINT NOT NULL,\
        first_seen_height BIGINT NOT NULL,\
        last_seen_height BIGINT NOT NULL\
    )",
    "CREATE INDEX IF NOT EXISTS ix_near_hos_staging_receipt_actions_tx_hash \
       ON near.near_hos_staging_receipt_actions (transaction_hash)",
    "CREATE TABLE IF NOT EXISTS near.near_hos_staging_receipt_edges (\
        parent_receipt_id TEXT NOT NULL,\
        child_receipt_id TEXT NOT NULL,\
        transaction_hash TEXT NOT NULL,\
        parent_execution_outcome_id TEXT NOT NULL,\
        block_height BIGINT NOT NULL,\
        block_hash TEXT NOT NULL,\
        block_timestamp BIGINT NOT NULL,\
        first_seen_height BIGINT NOT NULL,\
        last_seen_height BIGINT NOT NULL,\
        PRIMARY KEY (parent_receipt_id, child_receipt_id)\
    )",
    "CREATE INDEX IF NOT EXISTS ix_near_hos_staging_receipt_edges_tx_hash \
       ON near.near_hos_staging_receipt_edges (transaction_hash)",
    "CREATE TABLE IF NOT EXISTS near.near_hos_staging_receipt_status (\
        receipt_id TEXT PRIMARY KEY,\
        transaction_hash TEXT NOT NULL,\
        executed BOOLEAN NOT NULL DEFAULT FALSE,\
        first_seen_height BIGINT NOT NULL,\
        last_seen_height BIGINT NOT NULL\
    )",
    "CREATE INDEX IF NOT EXISTS ix_near_hos_staging_receipt_status_tx_hash \
       ON near.near_hos_staging_receipt_status (transaction_hash)",
    "CREATE INDEX IF NOT EXISTS ix_near_hos_staging_receipt_status_tx_pending \
       ON near.near_hos_staging_receipt_status (transaction_hash, executed)",
    "CREATE TABLE IF NOT EXISTS near.near_hos_staging_blocks (\
        transaction_hash TEXT NOT NULL,\
        height BIGINT NOT NULL,\
        hash TEXT NOT NULL,\
        prev_hash TEXT NOT NULL,\
        epoch_id TEXT NOT NULL,\
        next_epoch_id TEXT NOT NULL,\
        author TEXT NOT NULL,\
        timestamp BIGINT NOT NULL,\
        timestamp_nanosec BIGINT NOT NULL,\
        gas_price TEXT NOT NULL,\
        total_supply TEXT NOT NULL,\
        validator_reward TEXT NOT NULL,\
        protocol_version INTEGER NOT NULL,\
        chunks_included BIGINT NOT NULL,\
        block_ordinal BIGINT NULL,\
        prev_height BIGINT NULL,\
        raw_data JSONB NULL,\
        first_seen_height BIGINT NOT NULL,\
        last_seen_height BIGINT NOT NULL,\
        PRIMARY KEY (transaction_hash, height)\
    )",
    "CREATE INDEX IF NOT EXISTS ix_near_hos_staging_blocks_height \
       ON near.near_hos_staging_blocks (height)",
    "CREATE TABLE IF NOT EXISTS near.near_hos_staging_chunks (\
        transaction_hash TEXT NOT NULL,\
        chunk_hash TEXT NOT NULL,\
        block_height BIGINT NOT NULL,\
        shard_id BIGINT NOT NULL,\
        author TEXT NOT NULL,\
        height_created BIGINT NOT NULL,\
        height_included BIGINT NOT NULL,\
        gas_used BIGINT NOT NULL,\
        gas_limit BIGINT NOT NULL,\
        validator_reward TEXT NOT NULL,\
        raw_data JSONB NULL,\
        first_seen_height BIGINT NOT NULL,\
        last_seen_height BIGINT NOT NULL,\
        PRIMARY KEY (transaction_hash, chunk_hash)\
    )",
    "CREATE INDEX IF NOT EXISTS ix_near_hos_staging_chunks_tx_hash \
       ON near.near_hos_staging_chunks (transaction_hash)",
    "CREATE TABLE IF NOT EXISTS near.near_hos_staging_state_changes (\
        transaction_hash TEXT NOT NULL,\
        block_height BIGINT NOT NULL,\
        shard_id BIGINT NOT NULL,\
        state_changes JSONB NOT NULL,\
        first_seen_height BIGINT NOT NULL,\
        last_seen_height BIGINT NOT NULL,\
        PRIMARY KEY (transaction_hash, block_height, shard_id)\
    )",
    "CREATE INDEX IF NOT EXISTS ix_near_hos_staging_state_changes_tx_hash \
       ON near.near_hos_staging_state_changes (transaction_hash)",
];

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        for sql in DROP_HOS_STAGING_SCHEMA {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        for sql in RESTORE_HOS_STAGING_SCHEMA {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }
}
