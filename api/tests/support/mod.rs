#![allow(dead_code)]

use api::{routes, state::AppState};
use axum::Router;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use std::sync::Arc;

pub use test_support::{init_test_tracing, TestDatabase};

/// Build the API router with optional database connection for tests
pub fn build_router_with_state(db: Option<Arc<DatabaseConnection>>) -> Router {
    let state = AppState::new(db);
    Router::new()
        .nest("/api/v1", routes::create_routes())
        .nest("/api/agora", routes::agora::agora_routes())
        .merge(routes::health::health_routes())
        .with_state(state)
}

/// Provision a disposable PostgreSQL instance and router for e2e tests
pub async fn router_with_database() -> (Router, TestDatabase) {
    init_test_tracing();
    let database = TestDatabase::new().await;
    let conn = database.db();
    let router = build_router_with_state(Some(conn.clone()));
    (router, database)
}

pub async fn seed_venear_data(db: &impl ConnectionTrait) -> anyhow::Result<()> {
    execute_sql(
        db,
        r#"
        INSERT INTO near.near_receipt_actions (
            id,
            block_height,
            receipt_id,
            execution_outcome_id,
            transaction_hash,
            shard_id,
            signer_account_id,
            signer_public_key,
            gas_price,
            action_kind,
            action_json,
            predecessor_id,
            receiver_id,
            block_hash,
            chunk_hash,
            author,
            method_name,
            gas,
            deposit,
            args_base64,
            args_json,
            action_index,
            block_timestamp
        ) VALUES
            ('a-config-new-vote', 90, 'rcpt-config-new-vote', 'exec-config-new-vote', 'tx-config-new-vote', 0, 'owner.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"new"}}'::jsonb, 'owner.near', 'vote.dao', 'bh-90', 'ch-90', 'author.near', 'new', 0, '0', NULL, '{"config":{"guardians":["guardian1.near"],"reviewer_ids":["reviewer1.near","reviewer2.near"]}}'::jsonb, 0, 1759999900000000000),
            ('a-config-new-venear', 91, 'rcpt-config-new-venear', 'exec-config-new-venear', 'tx-config-new-venear', 0, 'owner.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"new"}}'::jsonb, 'owner.near', 'venear.dao', 'bh-91', 'ch-91', 'author.near', 'new', 0, '0', NULL, '{"config":{"guardians":["guardian0.near"]}}'::jsonb, 0, 1759999910000000000),
            ('a-p1-create', 100, 'rcpt-p1-create', 'exec-p1-create', 'tx-p1-create', 0, 'alice.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"create_proposal"}}'::jsonb, 'alice.near', 'vote.dao', 'bh-100', 'ch-100', 'author.near', 'create_proposal', 0, '0', NULL, '{"metadata":{"title":"P1","description":"Proposal 1","voting_options":["For","Against","Abstain"]}}'::jsonb, 0, 1760000000000000000),
            ('a-vote-lane', 110, 'rcpt-vote-1', 'exec-vote-1', 'tx-vp-2', 0, 'system.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"vote"}}'::jsonb, 'lane.near', 'vote.dao', 'bh-110', 'ch-110', 'author.near', 'vote', 0, '0', NULL, '{"proposal_id":"1","vote":"1"}'::jsonb, 0, 1760000100000000000),
            ('a-vote-jane', 111, 'rcpt-vote-2', 'exec-vote-2', 'tx-vote-2', 0, 'system.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"vote"}}'::jsonb, 'jane.near', 'vote.dao', 'bh-111', 'ch-111', 'author.near', 'vote', 0, '0', NULL, '{"proposal_id":"1","vote":"0"}'::jsonb, 0, 1760000110000000000),
            ('a-p1-approve', 102, 'rcpt-p1-approve', 'exec-p1-approve', 'tx-p1-final', 0, 'reviewer.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"approve_proposal"}}'::jsonb, 'reviewer.near', 'vote.dao', 'bh-102', 'ch-102', 'author.near', 'approve_proposal', 0, '0', NULL, '{"proposal_id":"1"}'::jsonb, 0, 1760000200000000000),
            ('a-p1-snapshot', 103, 'rcpt-p1-snapshot', 'exec-p1-snapshot', 'tx-p1-final', 0, 'vote.dao', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"on_get_snapshot"}}'::jsonb, 'vote.dao', 'vote.dao', 'bh-103', 'ch-103', 'author.near', 'on_get_snapshot', 0, '0', NULL, '{}'::jsonb, 0, 1760000210000000000),
            ('a-p2-create', 120, 'rcpt-p2-create', 'exec-p2-create', 'tx-p2-create', 1, 'bob.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"create_proposal"}}'::jsonb, 'bob.near', 'vote.dao', 'bh-120', 'ch-120', 'author.near', 'create_proposal', 0, '0', NULL, '{"metadata":{"title":"P2","description":"Proposal 2","voting_options":["For","Against","Abstain"]}}'::jsonb, 0, 1760000300000000000),
            ('a-lockup-lane', 129, 'rcpt-lockup-lane', 'exec-lockup-lane', 'tx-lockup-lane', 0, 'system.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"on_lockup_deployed"}}'::jsonb, 'venear.dao', 'venear.dao', 'bh-129', 'ch-129', 'author.near', 'on_lockup_deployed', 0, '0', NULL, '{"account_id":"lane.near"}'::jsonb, 0, 1760000390000000000),
            ('a-storage-lane', 130, 'rcpt-storage-lane', 'exec-storage-lane', 'tx-vp-1', 0, 'lane.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"storage_deposit"}}'::jsonb, 'lane.near', 'venear.dao', 'bh-130', 'ch-130', 'author.near', 'storage_deposit', 0, '0', NULL, '{"amount":"8"}'::jsonb, 0, 1760000400000000000),
            ('a-lockup-jane', 131, 'rcpt-lockup-jane', 'exec-lockup-jane', 'tx-lockup-jane', 0, 'system.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"on_lockup_deployed"}}'::jsonb, 'venear.dao', 'venear.dao', 'bh-131', 'ch-131', 'author.near', 'on_lockup_deployed', 0, '0', NULL, '{"account_id":"jane.near"}'::jsonb, 0, 1760000410000000000),
            ('a-delegate-lane', 143, 'rcpt-deleg-1', 'exec-deleg-1', 'tx-deleg-1', 0, 'system.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"delegate_all"}}'::jsonb, 'lane.near', 'venear.dao', 'bh-143', 'ch-143', 'author.near', 'delegate_all', 0, '0', NULL, '{"receiver_id":"jane.near"}'::jsonb, 0, 1760000530000000000),
            ('a-burn-lane', 143, 'rcpt-deleg-1', 'exec-deleg-1', 'tx-deleg-1', 0, 'lane.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"lock_near"}}'::jsonb, 'lane.near', 'venear.dao', 'bh-143', 'ch-143', 'author.near', 'lock_near', 0, '0', NULL, '{"amount":"3"}'::jsonb, 1, 1760000530000000000),
            ('a-config-set-guardians-vote', 160, 'rcpt-config-set-guardians-vote', 'exec-config-set-guardians-vote', 'tx-config-set-guardians-vote', 0, 'owner.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"set_guardians"}}'::jsonb, 'owner.near', 'vote.dao', 'bh-160', 'ch-160', 'author.near', 'set_guardians', 0, '0', NULL, '{"guardians":["guardian2.near","guardian3.near"]}'::jsonb, 0, 1760000600000000000),
            ('a-config-set-reviewers-vote', 170, 'rcpt-config-set-reviewers-vote', 'exec-config-set-reviewers-vote', 'tx-config-set-reviewers-vote', 0, 'owner.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"set_reviewer_ids"}}'::jsonb, 'owner.near', 'vote.dao', 'bh-170', 'ch-170', 'author.near', 'set_reviewer_ids', 0, '0', NULL, '{"reviewer_ids":["reviewer9.near"]}'::jsonb, 0, 1760000700000000000),
            ('a-config-set-guardians-venear', 171, 'rcpt-config-set-guardians-venear', 'exec-config-set-guardians-venear', 'tx-config-set-guardians-venear', 0, 'owner.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"set_guardians"}}'::jsonb, 'owner.near', 'venear.dao', 'bh-171', 'ch-171', 'author.near', 'set_guardians', 0, '0', NULL, '{"guardians":["guardian4.near"]}'::jsonb, 0, 1760000710000000000)
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO near.near_execution_outcomes (
            id,
            block_height,
            receipt_id,
            outcome,
            raw_data,
            shard_id,
            transaction_hash,
            block_hash,
            chunk_hash,
            gas_burnt,
            gas_used,
            tokens_burnt,
            executor_account_id,
            status,
            outcome_receipt_ids,
            executed_in_block_hash,
            logs,
            results_json,
            block_timestamp
        ) VALUES
            ('exec-config-new-vote', 90, 'rcpt-config-new-vote', '{}'::jsonb, NULL, 0, 'tx-config-new-vote', 'bh-90', 'ch-90', 0, 0, 0, 'vote.dao', 'SuccessValue', '[]'::jsonb, 'bh-90', '[]'::jsonb, NULL, 1759999900000000000),
            ('exec-config-new-venear', 91, 'rcpt-config-new-venear', '{}'::jsonb, NULL, 0, 'tx-config-new-venear', 'bh-91', 'ch-91', 0, 0, 0, 'venear.dao', 'SuccessValue', '[]'::jsonb, 'bh-91', '[]'::jsonb, NULL, 1759999910000000000),
            ('exec-p1-create', 100, 'rcpt-p1-create', '{}'::jsonb, NULL, 0, 'tx-p1-create', 'bh-100', 'ch-100', 0, 0, 0, 'vote.dao', 'SuccessValue', '[]'::jsonb, 'bh-100', '["EVENT_JSON:{\"event\":\"create_proposal\",\"data\":[{\"proposal_id\":\"1\"}]}"]'::jsonb, NULL, 1760000000000000000),
            ('exec-vote-1', 110, 'rcpt-vote-1', '{}'::jsonb, NULL, 0, 'tx-vp-2', 'bh-110', 'ch-110', 0, 0, 0, 'vote.dao', 'SuccessValue', '[]'::jsonb, 'bh-110', '["EVENT_JSON:{\"event\":\"add_vote\",\"data\":[{\"account_balance\":\"8\"}]}"]'::jsonb, NULL, 1760000100000000000),
            ('exec-vote-2', 111, 'rcpt-vote-2', '{}'::jsonb, NULL, 0, 'tx-vote-2', 'bh-111', 'ch-111', 0, 0, 0, 'vote.dao', 'SuccessValue', '[]'::jsonb, 'bh-111', '["EVENT_JSON:{\"event\":\"add_vote\",\"data\":[{\"account_balance\":\"900000000000000000000\"}]}"]'::jsonb, NULL, 1760000110000000000),
            ('exec-p1-approve', 102, 'rcpt-p1-approve', '{}'::jsonb, NULL, 0, 'tx-p1-final', 'bh-102', 'ch-102', 0, 0, 0, 'vote.dao', 'SuccessReceiptId', '[]'::jsonb, 'bh-102', '[]'::jsonb, '{"receipt_id":"rcpt-p1-snapshot"}'::jsonb, 1760000200000000000),
            ('exec-p1-snapshot', 103, 'rcpt-p1-snapshot', '{}'::jsonb, NULL, 0, 'tx-p1-final', 'bh-103', 'ch-103', 0, 0, 0, 'vote.dao', 'SuccessValue', '[]'::jsonb, 'bh-103', '[]'::jsonb, '{"creation_time_ns":"1760000000000000000","voting_start_time_ns":"1760000200000000000","voting_duration_ns":"3600000000000","snapshot_and_state":{"total_venear":"1000000000000000000"}}'::jsonb, 1760000210000000000),
            ('exec-p2-create', 120, 'rcpt-p2-create', '{}'::jsonb, NULL, 1, 'tx-p2-create', 'bh-120', 'ch-120', 0, 0, 0, 'vote.dao', 'SuccessValue', '[]'::jsonb, 'bh-120', '["EVENT_JSON:{\"event\":\"create_proposal\",\"data\":[{\"proposal_id\":\"2\"}]}"]'::jsonb, NULL, 1760000300000000000),
            ('exec-lockup-lane', 129, 'rcpt-lockup-lane', '{}'::jsonb, NULL, 0, 'tx-lockup-lane', 'bh-129', 'ch-129', 0, 0, 0, 'venear.dao', 'SuccessValue', '[]'::jsonb, 'bh-129', '["EVENT_JSON:{\"event\":\"lockup_deployed\",\"data\":[{\"account_id\":\"lane.near\"}]}"]'::jsonb, NULL, 1760000390000000000),
            ('exec-storage-lane', 130, 'rcpt-storage-lane', '{}'::jsonb, NULL, 0, 'tx-vp-1', 'bh-130', 'ch-130', 0, 0, 0, 'venear.dao', 'SuccessValue', '[]'::jsonb, 'bh-130', '["EVENT_JSON:{\"event\":\"ft_mint\",\"data\":[{\"amount\":\"8\",\"account_balance\":\"8\"}]}"]'::jsonb, NULL, 1760000400000000000),
            ('exec-lockup-jane', 131, 'rcpt-lockup-jane', '{}'::jsonb, NULL, 0, 'tx-lockup-jane', 'bh-131', 'ch-131', 0, 0, 0, 'venear.dao', 'SuccessValue', '[]'::jsonb, 'bh-131', '["EVENT_JSON:{\"event\":\"lockup_deployed\",\"data\":[{\"account_id\":\"jane.near\"}]}"]'::jsonb, NULL, 1760000410000000000),
            ('exec-deleg-1', 143, 'rcpt-deleg-1', '{}'::jsonb, NULL, 0, 'tx-deleg-1', 'bh-143', 'ch-143', 0, 0, 0, 'venear.dao', 'SuccessValue', '[]'::jsonb, 'bh-143', '["EVENT_JSON:{\"event\":\"ft_burn\",\"data\":[{\"owner_id\":\"lane.near\",\"amount\":\"3\"}]}"]'::jsonb, NULL, 1760000530000000000),
            ('exec-config-set-guardians-vote', 160, 'rcpt-config-set-guardians-vote', '{}'::jsonb, NULL, 0, 'tx-config-set-guardians-vote', 'bh-160', 'ch-160', 0, 0, 0, 'vote.dao', 'SuccessValue', '[]'::jsonb, 'bh-160', '[]'::jsonb, NULL, 1760000600000000000),
            ('exec-config-set-reviewers-vote', 170, 'rcpt-config-set-reviewers-vote', '{}'::jsonb, NULL, 0, 'tx-config-set-reviewers-vote', 'bh-170', 'ch-170', 0, 0, 0, 'vote.dao', 'SuccessValue', '[]'::jsonb, 'bh-170', '[]'::jsonb, NULL, 1760000700000000000),
            ('exec-config-set-guardians-venear', 171, 'rcpt-config-set-guardians-venear', '{}'::jsonb, NULL, 0, 'tx-config-set-guardians-venear', 'bh-171', 'ch-171', 0, 0, 0, 'venear.dao', 'SuccessValue', '[]'::jsonb, 'bh-171', '[]'::jsonb, NULL, 1760000710000000000)
        "#,
    )
    .await?;

    execute_sql(db, "SELECT near.refresh_hos_materialized_views()").await?;

    Ok(())
}

pub async fn execute_sql(db: &impl ConnectionTrait, sql: &str) -> anyhow::Result<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        vec![],
    ))
    .await?;

    Ok(())
}
