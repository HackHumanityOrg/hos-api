use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared("DROP TABLE IF EXISTS near.hos_config CASCADE")
            .await?;

        db.execute_unprepared(
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
        )
        .await?;

        db.execute_unprepared(
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
        )
        .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();

        db.execute_unprepared(
            "CREATE TABLE IF NOT EXISTS near.hos_config (
                id SMALLINT PRIMARY KEY,
                hos_contract TEXT NOT NULL,
                venear_contract_prefix TEXT NOT NULL,
                voting_contract_prefix TEXT NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                CONSTRAINT ck_near_hos_config_singleton CHECK (id = 1)
            )",
        )
        .await?;

        db.execute_unprepared(
            "INSERT INTO near.hos_config (id, hos_contract, venear_contract_prefix, voting_contract_prefix)
             VALUES (1, 'dao', 'venear', 'vote')
             ON CONFLICT (id) DO NOTHING",
        )
        .await?;

        db.execute_unprepared(
            "CREATE OR REPLACE FUNCTION near.is_hos_contract(account_id TEXT)
             RETURNS BOOLEAN
             LANGUAGE SQL
             STABLE
             AS $$
               SELECT CASE
                 WHEN account_id IS NULL OR btrim(account_id) = '' THEN FALSE
                 ELSE EXISTS (
                   SELECT 1
                   FROM near.hos_config cfg
                   WHERE account_id = cfg.hos_contract
                      OR account_id = cfg.venear_contract_prefix || '.' || cfg.hos_contract
                      OR account_id = cfg.voting_contract_prefix || '.' || cfg.hos_contract
                      OR account_id LIKE 'lockup.%.' || cfg.venear_contract_prefix || '.' || cfg.hos_contract
                      OR account_id LIKE 'lockup.%.' || cfg.voting_contract_prefix || '.' || cfg.hos_contract
                 )
               END
             $$",
        )
        .await?;

        db.execute_unprepared(
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
        )
        .await?;

        Ok(())
    }
}
