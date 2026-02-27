use api::types::{
    VenearAccountBalancesListResponse, VenearLockupActionsListResponse, VenearLockupsListResponse,
    VenearProposalEventsListResponse, VenearProposalsListResponse, VenearVotesListResponse,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::de::DeserializeOwned;
use tower::ServiceExt;

#[path = "../support/mod.rs"]
mod support;

#[tokio::test]
async fn venear_endpoints_return_seeded_data() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;
    seed_venear_data(database.db().as_ref())
        .await
        .expect("seed venear data");

    let proposals_page_1: VenearProposalsListResponse =
        get_json(&router, "/api/v1/venear/proposals?limit=1").await;
    assert_eq!(proposals_page_1.data.len(), 1);
    assert!(proposals_page_1.has_more);
    assert_eq!(proposals_page_1.data[0].proposal_id, 2);
    let cursor = proposals_page_1.next_cursor.clone().expect("next cursor");

    let proposals_page_2: VenearProposalsListResponse =
        get_json(&router, &format!("/api/v1/venear/proposals?cursor={cursor}"))
            .await;
    assert_eq!(proposals_page_2.data.len(), 1);
    assert!(!proposals_page_2.has_more);
    assert_eq!(proposals_page_2.data[0].proposal_id, 1);
    assert_eq!(
        proposals_page_2.data[0].latest_event_type,
        "proposal_approve_finalized"
    );
    assert_eq!(
        proposals_page_2.data[0].snapshot_total_venear.as_deref(),
        Some("1000000000000000000")
    );

    let proposal_events: VenearProposalEventsListResponse =
        get_json(&router, "/api/v1/venear/proposals/1/events").await;
    assert_eq!(proposal_events.data.len(), 2);
    assert_eq!(proposal_events.data[0].event_type, "proposal_approve_finalized");

    let votes: VenearVotesListResponse = get_json(&router, "/api/v1/venear/proposals/1/votes").await;
    assert_eq!(votes.data.len(), 2);
    assert_eq!(votes.data[0].account_balance, "900000000000000000000");

    let balances: VenearAccountBalancesListResponse =
        get_json(&router, "/api/v1/venear/accounts/lane.near/balances").await;
    assert_eq!(balances.data.len(), 2);
    assert_eq!(balances.data[0].event_type, "ft_burn");
    assert_eq!(balances.data[0].total_balance, "8");

    let lockups: VenearLockupsListResponse =
        get_json(&router, "/api/v1/venear/lockups?owner_account_id=lane.near").await;
    assert_eq!(lockups.data.len(), 1);
    assert_eq!(lockups.data[0].lockup_account_id, "lock1.venear.dao");

    let actions: VenearLockupActionsListResponse = get_json(
        &router,
        "/api/v1/venear/lockups/lock1.venear.dao/actions?method_name=select_staking_pool",
    )
    .await;
    assert_eq!(actions.data.len(), 1);
    assert_eq!(actions.data[0].method_name, "select_staking_pool");
}

async fn get_json<T: DeserializeOwned>(router: &Router, path: &str) -> T {
    let response = router
        .clone()
        .into_service()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::OK);

    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    serde_json::from_slice(&bytes).expect("json body")
}

async fn seed_venear_data(db: &impl ConnectionTrait) -> anyhow::Result<()> {
    execute_sql(
        db,
        r#"
        INSERT INTO near.near_venear_proposal_events (
            proposal_id, event_type, account_id, proposer_id, reviewer_id,
            voting_start_time_sec, snapshot_block_height, snapshot_root, snapshot_total_venear,
            metadata, payload, block_height, block_timestamp, shard_id,
            receipt_id, execution_outcome_id, predecessor_account_id, contract_id,
            log_index, data_index
        ) VALUES
            (
                1, 'create_proposal', NULL, 'alice.near', NULL,
                NULL, NULL, NULL, NULL,
                '{"title":"P1"}'::jsonb, '{"source":"intent"}'::jsonb,
                100, 1760000000000000000, 0,
                'rcpt-p1-create', 'exec-p1-create', 'alice.near', 'venear.dao',
                0, 0
            ),
            (
                1, 'proposal_approve_finalized', NULL, NULL, 'reviewer.near',
                1760000200, 176000050, 'root-1', '1000000000000000000',
                '{"title":"P1","status":"Approved"}'::jsonb, '{"source":"callback"}'::jsonb,
                102, 1760000200000000000, 0,
                'rcpt-p1-final', 'exec-p1-final', 'vote.dao', 'vote.dao',
                -1, 0
            ),
            (
                2, 'create_proposal', NULL, 'bob.near', NULL,
                NULL, NULL, NULL, NULL,
                '{"title":"P2"}'::jsonb, '{"source":"intent"}'::jsonb,
                120, 1760000300000000000, 1,
                'rcpt-p2-create', 'exec-p2-create', 'bob.near', 'venear.dao',
                0, 0
            )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO near.near_venear_votes (
            proposal_id, account_id, vote, account_balance, event_type, payload,
            block_height, block_timestamp, shard_id,
            receipt_id, execution_outcome_id, predecessor_account_id, contract_id,
            log_index, data_index
        ) VALUES
            (
                1, 'lane.near', 1, '32279877702596545200000002', 'add_vote', '{"from":"fixture"}'::jsonb,
                110, 1760000100000000000, 0,
                'rcpt-vote-1', 'exec-vote-1', 'lane.near', 'vote.dao',
                0, 0
            ),
            (
                1, 'jane.near', 0, '900000000000000000000', 'add_vote', NULL,
                111, 1760000110000000000, 0,
                'rcpt-vote-2', 'exec-vote-2', 'jane.near', 'vote.dao',
                0, 1
            )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO near.near_venear_account_balances (
            account_id, event_type, delta_amount, total_balance, locked_near_balance,
            lockup_update_nonce, lockup_version,
            block_height, block_timestamp, shard_id,
            receipt_id, execution_outcome_id, predecessor_account_id, contract_id,
            log_index, data_index, payload
        ) VALUES
            (
                'lane.near', 'ft_mint', '10', '10', NULL,
                NULL, NULL,
                130, 1760000400000000000, 0,
                'rcpt-bal-1', 'exec-bal-1', 'venear.dao', 'venear.dao',
                0, 0, '{"memo":"mint"}'::jsonb
            ),
            (
                'lane.near', 'ft_burn', '-2', '8', NULL,
                NULL, NULL,
                131, 1760000410000000000, 0,
                'rcpt-bal-2', 'exec-bal-2', 'venear.dao', 'venear.dao',
                0, 0, '{"memo":"burn"}'::jsonb
            )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO near.near_venear_lockups (
            lockup_account_id, owner_account_id, lockup_version, discovery_source,
            discovered_at_height, discovered_at_timestamp,
            last_seen_height, last_seen_timestamp,
            is_active, deleted_at_height, deleted_at_timestamp, metadata
        ) VALUES
            (
                'lock1.venear.dao', 'lane.near', 1, 'lockup_deployed_event',
                90, 1760000000000000000,
                140, 1760000500000000000,
                true, NULL, NULL, '{"origin":"fixture"}'::jsonb
            ),
            (
                'lock2.venear.dao', 'bob.near', 1, 'lockup_delete_event',
                80, 1759990000000000000,
                100, 1760000000000000000,
                false, 100, 1760000000000000000, NULL
            )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO near.near_venear_lockup_actions (
            lockup_account_id, owner_account_id,
            predecessor_account_id, receiver_account_id,
            method_name, action_source, action_index,
            args_json, args_base64, attached_deposit,
            transaction_hash, receipt_id, execution_outcome_id, success,
            block_height, block_timestamp, shard_id,
            staking_pool_account_id, metadata
        ) VALUES
            (
                'lock1.venear.dao', 'lane.near',
                'lane.near', 'lock1.venear.dao',
                'select_staking_pool', 'transaction', 0,
                '{"staking_pool_account_id":"poolv1.near"}'::jsonb, 'e30=', '0',
                'tx-hash-1', 'rcpt-action-1', 'exec-action-1', true,
                141, 1760000510000000000, 0,
                'poolv1.near', '{"source":"fixture"}'::jsonb
            ),
            (
                'lock1.venear.dao', 'lane.near',
                'lane.near', 'lock1.venear.dao',
                'unbond', 'receipt', 1,
                NULL, 'e30=', '1',
                NULL, 'rcpt-action-2', 'exec-action-2', true,
                142, 1760000520000000000, 0,
                NULL, NULL
            )
        "#,
    )
    .await?;

    Ok(())
}

async fn execute_sql(db: &impl ConnectionTrait, sql: &str) -> anyhow::Result<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        vec![],
    ))
    .await?;

    Ok(())
}
