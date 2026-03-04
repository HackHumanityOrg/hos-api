use api::types::{
    VenearDelegateVotingPowerEventsListResponse, VenearDelegatesListResponse,
    VenearDelegationEventsListResponse, VenearProposalDto, VenearProposalEventsListResponse,
    VenearProposalsListResponse, VenearVotesListResponse,
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
    assert_eq!(proposals_page_2.data[0].status, "approved");

    let created_only: VenearProposalsListResponse =
        get_json(&router, "/api/v1/venear/proposals?latest_event_type=create_proposal").await;
    assert_eq!(created_only.data.len(), 1);
    assert_eq!(created_only.data[0].proposal_id, 2);

    let proposal: VenearProposalDto = get_json(&router, "/api/v1/venear/proposals/1").await;
    assert_eq!(proposal.proposal_id, 1);
    assert_eq!(proposal.status, "approved");
    assert_eq!(proposal.reviewer_id.as_deref(), Some("reviewer.near"));
    assert_eq!(proposal.vote_aggregates.len(), 2);

    let proposal_events: VenearProposalEventsListResponse =
        get_json(&router, "/api/v1/venear/proposals/1/events").await;
    assert_eq!(proposal_events.data.len(), 2);
    assert_eq!(proposal_events.data[0].event_type, "proposal_approve_finalized");

    let finalized_events: VenearProposalEventsListResponse = get_json(
        &router,
        "/api/v1/venear/proposals/1/events?event_type=proposal_approve_finalized&actor_account_id=reviewer.near",
    )
    .await;
    assert_eq!(finalized_events.data.len(), 1);
    assert_eq!(finalized_events.data[0].event_type, "proposal_approve_finalized");

    let votes: VenearVotesListResponse = get_json(&router, "/api/v1/venear/proposals/1/votes").await;
    assert_eq!(votes.data.len(), 2);
    assert_eq!(votes.data[0].voter_account_id, "jane.near");
    assert_eq!(votes.data[0].voting_power, "900000000000000000000");
    assert!(votes.data[0].is_effective);

    let jane_votes: VenearVotesListResponse = get_json(
        &router,
        "/api/v1/venear/proposals/1/votes?voter_account_id=jane.near&vote_option=0",
    )
    .await;
    assert_eq!(jane_votes.data.len(), 1);
    assert_eq!(jane_votes.data[0].voter_account_id, "jane.near");
    assert_eq!(jane_votes.data[0].vote_option, 0);

    let delegates: VenearDelegatesListResponse =
        get_json(&router, "/api/v1/venear/delegates").await;
    assert_eq!(delegates.data.len(), 1);
    assert_eq!(delegates.data[0].account_id, "jane.near");
    assert_eq!(delegates.data[0].delegated_voting_power, "8");
    assert_eq!(delegates.data[0].total_voting_power, "8");
    assert!(delegates.data[0].has_inbound_delegation);

    let inbound_delegate: VenearDelegatesListResponse = get_json(
        &router,
        "/api/v1/venear/delegates?account_id_prefix=jan&has_inbound_delegation=true",
    )
    .await;
    assert_eq!(inbound_delegate.data.len(), 1);
    assert_eq!(inbound_delegate.data[0].account_id, "jane.near");

    let voting_power: VenearDelegateVotingPowerEventsListResponse =
        get_json(&router, "/api/v1/venear/delegates/lane.near/voting-power").await;
    assert_eq!(voting_power.data.len(), 3);
    assert_eq!(voting_power.data[0].account_id, "lane.near");
    assert_eq!(voting_power.data[0].event_type, "ft_burn");
    assert_eq!(voting_power.data[0].reason, "delegation_given");
    assert_eq!(voting_power.data[1].event_type, "add_vote");
    assert_eq!(voting_power.data[1].proposal_id, Some(1));
    assert_eq!(voting_power.data[1].voting_power, "8");
    assert_eq!(voting_power.data[1].reason, "vote_cast");
    assert_eq!(voting_power.data[2].event_type, "ft_mint");
    assert_eq!(voting_power.data[2].reason, "balance_increase");

    let proposal_scoped_vp: VenearDelegateVotingPowerEventsListResponse = get_json(
        &router,
        "/api/v1/venear/delegates/lane.near/voting-power?event_type=add_vote&proposal_id=1",
    )
    .await;
    assert_eq!(proposal_scoped_vp.data.len(), 1);
    assert_eq!(proposal_scoped_vp.data[0].event_type, "add_vote");
    assert_eq!(proposal_scoped_vp.data[0].proposal_id, Some(1));

    let jane_vp: VenearDelegateVotingPowerEventsListResponse =
        get_json(&router, "/api/v1/venear/delegates/jane.near/voting-power").await;
    assert_eq!(jane_vp.data.len(), 1);
    assert_eq!(jane_vp.data[0].account_id, "jane.near");
    assert_eq!(jane_vp.data[0].event_type, "ft_mint");
    assert_eq!(jane_vp.data[0].reason, "delegation_received");

    let delegations: VenearDelegationEventsListResponse =
        get_json(&router, "/api/v1/venear/delegates/lane.near/delegations").await;
    assert_eq!(delegations.data.len(), 1);
    assert_eq!(delegations.data[0].account_id, "lane.near");
    assert_eq!(delegations.data[0].direction, "outbound");
    assert_eq!(delegations.data[0].delegator_account_id, "lane.near");
    assert_eq!(
        delegations.data[0].delegatee_account_id.as_deref(),
        Some("jane.near")
    );
    assert!(delegations.data[0].success);

    let outbound_to_jane: VenearDelegationEventsListResponse = get_json(
        &router,
        "/api/v1/venear/delegates/lane.near/delegations?direction=outbound&counterparty_account_id=jane.near&event_type=delegate_all&success=true",
    )
    .await;
    assert_eq!(outbound_to_jane.data.len(), 1);
    assert_eq!(outbound_to_jane.data[0].delegator_account_id, "lane.near");
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
        INSERT INTO near.near_venear_account_voting_power_events (
            account_id, event_type, proposal_id, voting_power, delta_voting_power,
            block_height, block_timestamp, shard_id,
            receipt_id, execution_outcome_id, predecessor_account_id, contract_id,
            log_index, data_index, payload
        ) VALUES
            (
                'lane.near', 'ft_mint', NULL, '10', '10',
                130, 1760000400000000000, 0,
                'rcpt-vp-1', 'exec-vp-1', 'venear.dao', 'venear.dao',
                0, 0, '{"memo":"mint"}'::json
            ),
            (
                'lane.near', 'add_vote', 1, '8', '-2',
                132, 1760000420000000000, 0,
                'rcpt-vp-2', 'exec-vp-2', 'lane.near', 'vote.dao',
                0, 1, '{"memo":"vote"}'::json
            ),
            (
                'lane.near', 'ft_burn', NULL, '5', '-3',
                143, 1760000530000000000, 0,
                'rcpt-deleg-1', 'exec-deleg-1', 'venear.dao', 'venear.dao',
                0, 2, '{"memo":"delegation burn"}'::json
            ),
            (
                'jane.near', 'ft_mint', NULL, '3', '3',
                143, 1760000530000000000, 0,
                'rcpt-deleg-1', 'exec-deleg-1', 'venear.dao', 'venear.dao',
                0, 3, '{"memo":"delegation mint"}'::json
            )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO near.near_venear_lockup_events (
            lockup_account_id, owner_account_id, event_type,
            lockup_version, lockup_update_nonce, locked_near_balance,
            action_source, transaction_hash,
            receipt_id, execution_outcome_id,
            predecessor_account_id, contract_id,
            success, block_height, block_timestamp, shard_id,
            log_index, data_index, payload
        ) VALUES
            (
                'lock1.venear.dao', 'lane.near', 'lockup_deployed',
                1, NULL, NULL,
                'event_json', 'tx-lockup-1',
                'rcpt-lockup-1', 'exec-lockup-1',
                'venear.dao', 'venear.dao',
                true, 140, 1760000500000000000, 0,
                0, 0, '{"origin":"fixture"}'::jsonb
            ),
            (
                'lock2.venear.dao', 'bob.near', 'lockup_delete',
                1, NULL, NULL,
                'event_json', 'tx-lockup-2',
                'rcpt-lockup-2', 'exec-lockup-2',
                'bob.near', 'lock2.venear.dao',
                true, 100, 1760000000000000000, 0,
                0, 0, NULL
            )
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO near.near_venear_delegation_events (
            delegator_account_id, delegatee_account_id, event_type,
            action_source, action_index,
            args_json, args_base64, attached_deposit,
            transaction_hash, receipt_id, execution_outcome_id,
            success, block_height, block_timestamp, shard_id,
            contract_id, metadata
        ) VALUES
            (
                'lane.near', 'jane.near', 'delegate_all',
                'receipt', 0,
                '{"receiver_id":"jane.near"}'::jsonb, 'eyJyZWNlaXZlcl9pZCI6ImphbmUubmVhciJ9', '1',
                'tx-deleg-1', 'rcpt-deleg-1', 'exec-deleg-1',
                true, 143, 1760000530000000000, 0,
                'venear.dao', '{"source":"fixture"}'::jsonb
            ),
            (
                'jane.near', NULL, 'undelegate',
                'receipt', 1,
                '{}'::jsonb, 'e30=', '1',
                'tx-deleg-2', 'rcpt-deleg-2', 'exec-deleg-2',
                false, 144, 1760000540000000000, 0,
                'venear.dao', '{"source":"fixture"}'::jsonb
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
