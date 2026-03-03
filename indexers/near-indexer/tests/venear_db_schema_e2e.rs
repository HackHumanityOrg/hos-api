mod support;

use anyhow::Result;
use common::config::{
    AppConfig, DatabaseConfig, IndexerConfig, LoggingConfig, NearLakeConfig, NearModuleStreamMode,
    NearModulesConfig, NearStreamProvider, TelemetryConfig, VenearGovernanceModuleConfig,
};
use near_indexer::modules::venear_governance::VenearGovernanceModule;
use near_indexer::IndexerModule;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use support::{init_test_tracing, TestDatabase};

#[tokio::test]
async fn venear_schema_tables_exist_and_module_initializes() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();

    let module = VenearGovernanceModule::new(&build_test_config());
    module.initialize(conn.as_ref()).await?;

    let table_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value FROM information_schema.tables WHERE table_schema = 'near' AND table_name IN (
            'near_venear_events_raw',
            'near_venear_lockup_events',
            'near_venear_account_balances',
            'near_venear_account_voting_power_events',
            'near_venear_proposal_events',
            'near_venear_votes',
            'near_venear_lockup_actions',
            'near_venear_delegation_events',
            'near_venear_admin_events',
            'near_venear_governance_actions',
            'near_venear_unhandled_events'
        )",
    )
    .await?;

    assert_eq!(table_count, 11);

    Ok(())
}

#[tokio::test]
async fn venear_account_balances_store_cumulative_totals_and_dedup_identity() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_venear_account_balances (
            account_id, event_type, delta_amount, total_balance, locked_near_balance,
            lockup_update_nonce, lockup_version, block_height, block_timestamp, shard_id,
            receipt_id, execution_outcome_id, predecessor_account_id, contract_id,
            log_index, data_index, payload
        ) VALUES (
            'alice.near', 'ft_mint', '20', '20', NULL,
            NULL, NULL, 100, 1000, 0,
            'rcpt-1', 'exec-1', 'venear.dao', 'venear.dao',
            0, 0, NULL
        )",
    )
    .await?;

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_venear_account_balances (
            account_id, event_type, delta_amount, total_balance, locked_near_balance,
            lockup_update_nonce, lockup_version, block_height, block_timestamp, shard_id,
            receipt_id, execution_outcome_id, predecessor_account_id, contract_id,
            log_index, data_index, payload
        ) VALUES (
            'alice.near', 'ft_mint', '5', '25', NULL,
            NULL, NULL, 101, 1001, 0,
            'rcpt-2', 'exec-2', 'venear.dao', 'venear.dao',
            0, 0, NULL
        )",
    )
    .await?;

    let latest_total = scalar_string(
        conn.as_ref(),
        "SELECT total_balance AS value
         FROM near.near_venear_account_balances
         WHERE account_id = 'alice.near'
         ORDER BY block_height DESC, id DESC
         LIMIT 1",
    )
    .await?;
    assert_eq!(latest_total, "25");

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_venear_account_balances (
            account_id, event_type, delta_amount, total_balance, locked_near_balance,
            lockup_update_nonce, lockup_version, block_height, block_timestamp, shard_id,
            receipt_id, execution_outcome_id, predecessor_account_id, contract_id,
            log_index, data_index, payload
        ) VALUES (
            'alice.near', 'ft_mint', '20', '20', NULL,
            NULL, NULL, 100, 1000, 0,
            'rcpt-1', 'exec-1', 'venear.dao', 'venear.dao',
            0, 0, '{\"note\":\"updated\"}'::jsonb
        )
        ON CONFLICT (execution_outcome_id, log_index, data_index, account_id)
        DO UPDATE SET
            total_balance = EXCLUDED.total_balance,
            payload = EXCLUDED.payload",
    )
    .await?;

    let dedup_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_venear_account_balances
         WHERE account_id = 'alice.near' AND execution_outcome_id = 'exec-1'",
    )
    .await?;
    assert_eq!(dedup_count, 1);

    Ok(())
}

#[tokio::test]
async fn venear_proposal_events_allow_intent_and_callback_finalization() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_venear_proposal_events (
            proposal_id, event_type, account_id, proposer_id, reviewer_id,
            voting_start_time_sec, snapshot_block_height, snapshot_root,
            snapshot_total_venear, metadata, payload,
            block_height, block_timestamp, shard_id,
            receipt_id, execution_outcome_id, predecessor_account_id, contract_id,
            log_index, data_index
        ) VALUES (
            42, 'proposal_approve', 'alice.near', NULL, NULL,
            NULL, NULL, NULL,
            NULL, NULL, '{\"source\":\"intent\"}'::jsonb,
            200, 2000, 0,
            'rcpt-approve', 'exec-approve', 'vote.dao', 'venear.dao',
            0, 0
        )",
    )
    .await?;

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_venear_proposal_events (
            proposal_id, event_type, account_id, proposer_id, reviewer_id,
            voting_start_time_sec, snapshot_block_height, snapshot_root,
            snapshot_total_venear, metadata, payload,
            block_height, block_timestamp, shard_id,
            receipt_id, execution_outcome_id, predecessor_account_id, contract_id,
            log_index, data_index
        ) VALUES (
            42, 'proposal_approve_finalized', 'vote.dao', NULL, NULL,
            NULL, NULL, NULL,
            NULL, NULL, '{\"source\":\"callback\"}'::jsonb,
            201, 2001, 0,
            'rcpt-finalized', 'exec-finalized', 'vote.dao', 'vote.dao',
            -1, 0
        )",
    )
    .await?;

    let events_for_proposal = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_venear_proposal_events
         WHERE proposal_id = 42",
    )
    .await?;
    assert_eq!(events_for_proposal, 2);

    Ok(())
}

#[tokio::test]
async fn venear_delegation_events_upsert_on_identity_conflict() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_venear_delegation_events (
            delegator_account_id, delegatee_account_id, event_type,
            action_source, action_index,
            args_json, args_base64, attached_deposit,
            transaction_hash, receipt_id, execution_outcome_id, success,
            block_height, block_timestamp, shard_id,
            contract_id, metadata
        ) VALUES (
            'alice.near', 'bob.near', 'delegate_all',
            'receipt', 0,
            '{\"receiver_id\":\"bob.near\"}'::jsonb, 'e30=', '1',
            'tx-delegate-1', 'rcpt-delegate-1', 'exec-delegate-1', true,
            300, 3000, 0,
            'venear.dao', '{\"source\":\"first\"}'::jsonb
        )",
    )
    .await?;

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_venear_delegation_events (
            delegator_account_id, delegatee_account_id, event_type,
            action_source, action_index,
            args_json, args_base64, attached_deposit,
            transaction_hash, receipt_id, execution_outcome_id, success,
            block_height, block_timestamp, shard_id,
            contract_id, metadata
        ) VALUES (
            'alice.near', NULL, 'delegate_all',
            'receipt', 0,
            '{}'::jsonb, 'e30=', '1',
            'tx-delegate-1b', 'rcpt-delegate-1', 'exec-delegate-1b', false,
            301, 3001, 1,
            'venear.dao', '{\"source\":\"updated\"}'::jsonb
        )
        ON CONFLICT (receipt_id, action_index, event_type, delegator_account_id, contract_id)
        DO UPDATE SET
            delegatee_account_id = EXCLUDED.delegatee_account_id,
            execution_outcome_id = EXCLUDED.execution_outcome_id,
            success = EXCLUDED.success,
            block_height = EXCLUDED.block_height,
            metadata = EXCLUDED.metadata",
    )
    .await?;

    let dedup_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_venear_delegation_events
         WHERE receipt_id = 'rcpt-delegate-1'",
    )
    .await?;
    assert_eq!(dedup_count, 1);

    let success = scalar_i64(
        conn.as_ref(),
        "SELECT CASE WHEN success THEN 1 ELSE 0 END::bigint AS value
         FROM near.near_venear_delegation_events
         WHERE receipt_id = 'rcpt-delegate-1'",
    )
    .await?;
    assert_eq!(success, 0);

    Ok(())
}

#[tokio::test]
async fn venear_governance_actions_upsert_and_lockup_timestamp_column_work() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_venear_governance_actions (
            action_source, action_index, transaction_hash, receipt_id, execution_outcome_id,
            block_height, block_timestamp, shard_id,
            predecessor_account_id, receiver_account_id, contract_id, method_name,
            args_json, args_base64, attached_deposit, success, success_value_json,
            action_category, proposal_id, vote_option,
            delegator_account_id, delegatee_account_id, owner_account_id, metadata
        ) VALUES (
            'receipt', 0, 'tx-1', 'rcpt-1', 'exec-1',
            500, 5000, 0,
            'alice.near', 'vote.dao', 'vote.dao', 'vote',
            '{\"proposal_id\":1,\"vote\":0}'::jsonb, 'e30=', '1', true, NULL,
            'vote', 1, 0,
            NULL, NULL, NULL, '{\"source\":\"first\"}'::jsonb
        )",
    )
    .await?;

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_venear_governance_actions (
            action_source, action_index, transaction_hash, receipt_id, execution_outcome_id,
            block_height, block_timestamp, shard_id,
            predecessor_account_id, receiver_account_id, contract_id, method_name,
            args_json, args_base64, attached_deposit, success, success_value_json,
            action_category, proposal_id, vote_option,
            delegator_account_id, delegatee_account_id, owner_account_id, metadata
        ) VALUES (
            'receipt', 0, 'tx-1b', 'rcpt-1', 'exec-1b',
            501, 5001, 1,
            'alice.near', 'vote.dao', 'vote.dao', 'vote',
            '{\"proposal_id\":1,\"vote\":1}'::jsonb, 'e30=', '1', false, '{\"error\":\"x\"}'::jsonb,
            'vote', 1, 1,
            NULL, NULL, NULL, '{\"source\":\"updated\"}'::jsonb
        )
        ON CONFLICT (receipt_id, action_index, method_name, contract_id)
        DO UPDATE SET
            execution_outcome_id = EXCLUDED.execution_outcome_id,
            success = EXCLUDED.success,
            vote_option = EXCLUDED.vote_option,
            metadata = EXCLUDED.metadata",
    )
    .await?;

    let actions_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_venear_governance_actions
         WHERE receipt_id = 'rcpt-1' AND method_name = 'vote'",
    )
    .await?;
    assert_eq!(actions_count, 1);

    let vote_option = scalar_i64(
        conn.as_ref(),
        "SELECT vote_option::bigint AS value
         FROM near.near_venear_governance_actions
         WHERE receipt_id = 'rcpt-1' AND method_name = 'vote'",
    )
    .await?;
    assert_eq!(vote_option, 1);

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_venear_lockup_events (
            lockup_account_id, owner_account_id, event_type, lockup_version, lockup_update_nonce,
            lockup_timestamp_ns, locked_near_balance, action_source, transaction_hash,
            receipt_id, execution_outcome_id, predecessor_account_id, contract_id,
            success, block_height, block_timestamp, shard_id, log_index, data_index, payload
        ) VALUES (
            'abc.venear.dao', 'alice.near', 'lockup_update', 1, 10,
            1234567890, '1000', 'event_json', NULL,
            'rcpt-lock', 'exec-lock', 'abc.venear.dao', 'abc.venear.dao',
            true, 510, 5100, 0, 0, 0, NULL
        )",
    )
    .await?;

    let lockup_timestamp = scalar_i64(
        conn.as_ref(),
        "SELECT lockup_timestamp_ns AS value
         FROM near.near_venear_lockup_events
         WHERE receipt_id = 'rcpt-lock'",
    )
    .await?;
    assert_eq!(lockup_timestamp, 1234567890);

    Ok(())
}

#[tokio::test]
async fn venear_unhandled_events_upsert_on_identity_conflict() -> Result<()> {
    init_test_tracing();

    let db = TestDatabase::new().await;
    let conn = db.db();

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_venear_unhandled_events (
            block_height, block_timestamp, shard_id, receipt_id, execution_outcome_id,
            predecessor_account_id, contract_id, success, standard, event_type,
            log_index, data_index, event_data, log_text, reason
        ) VALUES (
            600, 6000, 0, 'rcpt-u1', 'exec-u1',
            'alice.near', 'venear.dao', true, 'venear', 'new_event',
            0, 0, '{\"x\":1}'::jsonb, 'EVENT_JSON:{...}', 'unmapped_event_type'
        )",
    )
    .await?;

    execute_sql(
        conn.as_ref(),
        "INSERT INTO near.near_venear_unhandled_events (
            block_height, block_timestamp, shard_id, receipt_id, execution_outcome_id,
            predecessor_account_id, contract_id, success, standard, event_type,
            log_index, data_index, event_data, log_text, reason
        ) VALUES (
            601, 6001, 1, 'rcpt-u1b', 'exec-u1',
            'alice.near', 'venear.dao', false, 'venear', 'new_event',
            0, 0, '{\"x\":2}'::jsonb, 'EVENT_JSON:{...}', 'unmapped_event_type'
        )
        ON CONFLICT (execution_outcome_id, log_index, data_index, contract_id, standard, event_type)
        DO UPDATE SET
            success = EXCLUDED.success,
            block_height = EXCLUDED.block_height,
            event_data = EXCLUDED.event_data",
    )
    .await?;

    let unhandled_count = scalar_i64(
        conn.as_ref(),
        "SELECT COUNT(*)::bigint AS value
         FROM near.near_venear_unhandled_events
         WHERE execution_outcome_id = 'exec-u1'",
    )
    .await?;
    assert_eq!(unhandled_count, 1);

    let success = scalar_i64(
        conn.as_ref(),
        "SELECT CASE WHEN success THEN 1 ELSE 0 END::bigint AS value
         FROM near.near_venear_unhandled_events
         WHERE execution_outcome_id = 'exec-u1'",
    )
    .await?;
    assert_eq!(success, 0);

    Ok(())
}

fn build_test_config() -> AppConfig {
    let db = DatabaseConfig {
        url: "postgres://unused".to_string(),
        ..DatabaseConfig::default()
    };

    let cfg = IndexerConfig {
        database: db,
        logging: LoggingConfig::default(),
        telemetry: TelemetryConfig::default(),
        discourse: None,
        telegram: None,
        near: Some(NearLakeConfig {
            provider: NearStreamProvider::Lake,
            chain: "mainnet".to_string(),
            start_height: 0,
            concurrency: Some(1),
            preload_pool_size: Some(1),
            fastnear_api_key: None,
            fastnear_num_threads: None,
            modules: Some(NearModulesConfig {
                raw: None,
                accounts: None,
                tokens: None,
                staking_pools: None,
                venear_governance: Some(VenearGovernanceModuleConfig {
                    enabled: true,
                    stream_mode: NearModuleStreamMode::Shared,
                    start_height: None,
                    venear_contract_id: "venear.dao".to_string(),
                    voting_contract_id: "vote.dao".to_string(),
                    lockup_account_suffix: ".venear.dao".to_string(),
                    known_lockups: vec![],
                }),
            }),
        }),
    };

    AppConfig::Indexer(Box::new(cfg))
}

async fn scalar_i64(db: &impl ConnectionTrait, sql: &str) -> Result<i64> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![],
        ))
        .await?
        .expect("expected one row");

    Ok(row.try_get("", "value")?)
}

async fn scalar_string(db: &impl ConnectionTrait, sql: &str) -> Result<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![],
        ))
        .await?
        .expect("expected one row");

    Ok(row.try_get("", "value")?)
}

async fn execute_sql(db: &impl ConnectionTrait, sql: &str) -> Result<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        vec![],
    ))
    .await?;

    Ok(())
}
