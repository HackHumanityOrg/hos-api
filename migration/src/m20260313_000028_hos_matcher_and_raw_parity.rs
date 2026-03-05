use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let statements: &[&str] = &[
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
            "CREATE OR REPLACE FUNCTION near.is_hos_contract(account_id TEXT)
             RETURNS BOOLEAN
             LANGUAGE SQL
             STABLE
             AS $$
               SELECT CASE
                 WHEN account_id IS NULL OR btrim(account_id) = '' THEN FALSE
                 ELSE
                   lower(account_id) = 'venear.dao'
                   OR lower(account_id) = 'vote.dao'
                   OR lower(account_id) = 'hos-root.sputnik-dao.near'
                   OR lower(account_id) = 'rewards.hos-dao.near'
                   OR lower(account_id) ~ '^[0-9a-f]{40}\\.venear\\.dao$'
               END
             $$",
            "CREATE OR REPLACE FUNCTION near.is_hos_action(receiver_id TEXT, predecessor_id TEXT, signer_account_id TEXT)
             RETURNS BOOLEAN
             LANGUAGE SQL
             STABLE
             AS $$
               SELECT
                 near.is_hos_contract(receiver_id)
                 OR near.is_hos_contract(predecessor_id)
                 OR near.is_hos_contract(signer_account_id)
             $$",
        ];

        for sql in statements {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        let statements: &[&str] = &[
            "DROP TABLE IF EXISTS near.near_hos_staging_state_changes",
            "DROP TABLE IF EXISTS near.near_hos_staging_chunks",
            "CREATE OR REPLACE FUNCTION near.is_hos_contract(account_id TEXT)
             RETURNS BOOLEAN
             LANGUAGE SQL
             STABLE
             AS $$
               SELECT CASE
                 WHEN account_id IS NULL OR btrim(account_id) = '' THEN FALSE
                 ELSE
                   account_id = 'dao'
                   OR account_id = 'venear.dao'
                   OR account_id = 'vote.dao'
                   OR account_id LIKE 'lockup.%.venear.dao'
                   OR account_id LIKE 'lockup.%.vote.dao'
               END
             $$",
            "CREATE OR REPLACE FUNCTION near.is_hos_action(receiver_id TEXT, predecessor_id TEXT, signer_account_id TEXT)
             RETURNS BOOLEAN
             LANGUAGE SQL
             STABLE
             AS $$
               SELECT
                 near.is_hos_contract(receiver_id)
                 OR near.is_hos_contract(predecessor_id)
                 OR near.is_hos_contract(signer_account_id)
             $$",
        ];

        for sql in statements {
            db.execute_unprepared(sql).await?;
        }

        Ok(())
    }
}
