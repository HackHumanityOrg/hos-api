use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let statements: &[&str] = &[
            // -- from m20251001_000002_near_schema --
            "ALTER TABLE near.near_transactions ALTER COLUMN actions TYPE jsonb USING actions::jsonb",
            "ALTER TABLE near.near_transactions ALTER COLUMN raw_data TYPE jsonb USING raw_data::jsonb",
            "ALTER TABLE near.near_receipts ALTER COLUMN receipt TYPE jsonb USING receipt::jsonb",
            "ALTER TABLE near.near_receipts ALTER COLUMN raw_data TYPE jsonb USING raw_data::jsonb",
            "ALTER TABLE near.near_execution_outcomes ALTER COLUMN outcome TYPE jsonb USING outcome::jsonb",
            "ALTER TABLE near.near_execution_outcomes ALTER COLUMN raw_data TYPE jsonb USING raw_data::jsonb",
            "ALTER TABLE near.near_state_changes ALTER COLUMN state_changes TYPE jsonb USING state_changes::jsonb",
            "ALTER TABLE near.near_access_keys ALTER COLUMN method_names TYPE jsonb USING method_names::jsonb",
            "ALTER TABLE near.near_account_events ALTER COLUMN metadata TYPE jsonb USING metadata::jsonb",
            "ALTER TABLE near.near_token_events ALTER COLUMN event_data TYPE jsonb USING event_data::jsonb",
            "ALTER TABLE near.near_token_contracts ALTER COLUMN standards_detected TYPE jsonb USING standards_detected::jsonb",
            "ALTER TABLE near.near_token_contracts ALTER COLUMN ft_metadata TYPE jsonb USING ft_metadata::jsonb",
            "ALTER TABLE near.near_token_contracts ALTER COLUMN nft_metadata TYPE jsonb USING nft_metadata::jsonb",
            "ALTER TABLE near.near_token_contracts ALTER COLUMN mt_metadata TYPE jsonb USING mt_metadata::jsonb",
            "ALTER TABLE near.near_token_balances ALTER COLUMN metadata TYPE jsonb USING metadata::jsonb",
            "ALTER TABLE near.near_token_metadata ALTER COLUMN metadata TYPE jsonb USING metadata::jsonb",
            "ALTER TABLE near.near_staking_pools ALTER COLUMN metadata TYPE jsonb USING metadata::jsonb",
            "ALTER TABLE near.near_staking_pool_actions ALTER COLUMN args_json TYPE jsonb USING args_json::jsonb",
            "ALTER TABLE near.near_staking_pool_events ALTER COLUMN event_data TYPE jsonb USING event_data::jsonb",
            "ALTER TABLE near.near_staking_epochs ALTER COLUMN validators_info TYPE jsonb USING validators_info::jsonb",
            "ALTER TABLE near.near_staking_epochs ALTER COLUMN chunk_producers_settlement TYPE jsonb USING chunk_producers_settlement::jsonb",
            // -- from m20260227_000005_near_venear_governance_schema --
            "ALTER TABLE near.near_venear_events_raw ALTER COLUMN event_data TYPE jsonb USING event_data::jsonb",
            "ALTER TABLE near.near_venear_lockups ALTER COLUMN metadata TYPE jsonb USING metadata::jsonb",
            "ALTER TABLE near.near_venear_account_balances ALTER COLUMN payload TYPE jsonb USING payload::jsonb",
            "ALTER TABLE near.near_venear_proposal_events ALTER COLUMN metadata TYPE jsonb USING metadata::jsonb",
            "ALTER TABLE near.near_venear_proposal_events ALTER COLUMN payload TYPE jsonb USING payload::jsonb",
            "ALTER TABLE near.near_venear_votes ALTER COLUMN payload TYPE jsonb USING payload::jsonb",
            "ALTER TABLE near.near_venear_lockup_actions ALTER COLUMN args_json TYPE jsonb USING args_json::jsonb",
            "ALTER TABLE near.near_venear_lockup_actions ALTER COLUMN metadata TYPE jsonb USING metadata::jsonb",
        ];
        for sql in statements {
            db.execute_unprepared(sql).await?;
        }
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        let statements: &[&str] = &[
            "ALTER TABLE near.near_transactions ALTER COLUMN actions TYPE json USING actions::text::json",
            "ALTER TABLE near.near_transactions ALTER COLUMN raw_data TYPE json USING raw_data::text::json",
            "ALTER TABLE near.near_receipts ALTER COLUMN receipt TYPE json USING receipt::text::json",
            "ALTER TABLE near.near_receipts ALTER COLUMN raw_data TYPE json USING raw_data::text::json",
            "ALTER TABLE near.near_execution_outcomes ALTER COLUMN outcome TYPE json USING outcome::text::json",
            "ALTER TABLE near.near_execution_outcomes ALTER COLUMN raw_data TYPE json USING raw_data::text::json",
            "ALTER TABLE near.near_state_changes ALTER COLUMN state_changes TYPE json USING state_changes::text::json",
            "ALTER TABLE near.near_access_keys ALTER COLUMN method_names TYPE json USING method_names::text::json",
            "ALTER TABLE near.near_account_events ALTER COLUMN metadata TYPE json USING metadata::text::json",
            "ALTER TABLE near.near_token_events ALTER COLUMN event_data TYPE json USING event_data::text::json",
            "ALTER TABLE near.near_token_contracts ALTER COLUMN standards_detected TYPE json USING standards_detected::text::json",
            "ALTER TABLE near.near_token_contracts ALTER COLUMN ft_metadata TYPE json USING ft_metadata::text::json",
            "ALTER TABLE near.near_token_contracts ALTER COLUMN nft_metadata TYPE json USING nft_metadata::text::json",
            "ALTER TABLE near.near_token_contracts ALTER COLUMN mt_metadata TYPE json USING mt_metadata::text::json",
            "ALTER TABLE near.near_token_balances ALTER COLUMN metadata TYPE json USING metadata::text::json",
            "ALTER TABLE near.near_token_metadata ALTER COLUMN metadata TYPE json USING metadata::text::json",
            "ALTER TABLE near.near_staking_pools ALTER COLUMN metadata TYPE json USING metadata::text::json",
            "ALTER TABLE near.near_staking_pool_actions ALTER COLUMN args_json TYPE json USING args_json::text::json",
            "ALTER TABLE near.near_staking_pool_events ALTER COLUMN event_data TYPE json USING event_data::text::json",
            "ALTER TABLE near.near_staking_epochs ALTER COLUMN validators_info TYPE json USING validators_info::text::json",
            "ALTER TABLE near.near_staking_epochs ALTER COLUMN chunk_producers_settlement TYPE json USING chunk_producers_settlement::text::json",
            "ALTER TABLE near.near_venear_events_raw ALTER COLUMN event_data TYPE json USING event_data::text::json",
            "ALTER TABLE near.near_venear_lockups ALTER COLUMN metadata TYPE json USING metadata::text::json",
            "ALTER TABLE near.near_venear_account_balances ALTER COLUMN payload TYPE json USING payload::text::json",
            "ALTER TABLE near.near_venear_proposal_events ALTER COLUMN metadata TYPE json USING metadata::text::json",
            "ALTER TABLE near.near_venear_proposal_events ALTER COLUMN payload TYPE json USING payload::text::json",
            "ALTER TABLE near.near_venear_votes ALTER COLUMN payload TYPE json USING payload::text::json",
            "ALTER TABLE near.near_venear_lockup_actions ALTER COLUMN args_json TYPE json USING args_json::text::json",
            "ALTER TABLE near.near_venear_lockup_actions ALTER COLUMN metadata TYPE json USING metadata::text::json",
        ];
        for sql in statements {
            db.execute_unprepared(sql).await?;
        }
        Ok(())
    }
}
