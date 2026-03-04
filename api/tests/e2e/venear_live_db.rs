use api::types::{
    VenearConfigCurrentResponse, VenearConfigHistoryListResponse,
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

    let proposals_page_2: VenearProposalsListResponse = get_json(
        &router,
        &format!("/api/v1/venear/proposals?cursor={cursor}"),
    )
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
    assert_eq!(proposals_page_2.data[0].status, "finished");

    let created_only: VenearProposalsListResponse = get_json(
        &router,
        "/api/v1/venear/proposals?latest_event_type=create_proposal",
    )
    .await;
    assert_eq!(created_only.data.len(), 1);
    assert_eq!(created_only.data[0].proposal_id, 2);

    let config_current: VenearConfigCurrentResponse =
        get_json(&router, "/api/v1/venear/config/current").await;
    assert_eq!(config_current.data.len(), 3);

    let vote_guardians = config_current
        .data
        .iter()
        .find(|row| row.contract_id == "vote.dao" && row.field == "guardians")
        .expect("vote.dao guardians current row");
    assert_eq!(
        vote_guardians.value,
        vec!["guardian2.near".to_string(), "guardian3.near".to_string()]
    );
    assert_eq!(vote_guardians.source, "set_method");

    let vote_reviewers = config_current
        .data
        .iter()
        .find(|row| row.contract_id == "vote.dao" && row.field == "reviewer_ids")
        .expect("vote.dao reviewer_ids current row");
    assert_eq!(vote_reviewers.value, vec!["reviewer9.near".to_string()]);
    assert_eq!(vote_reviewers.source, "set_method");

    let venear_guardians = config_current
        .data
        .iter()
        .find(|row| row.contract_id == "venear.dao" && row.field == "guardians")
        .expect("venear.dao guardians current row");
    assert_eq!(venear_guardians.value, vec!["guardian4.near".to_string()]);
    assert_eq!(venear_guardians.source, "set_method");

    let config_history_page_1: VenearConfigHistoryListResponse =
        get_json(&router, "/api/v1/venear/config/history?limit=2").await;
    assert_eq!(config_history_page_1.data.len(), 2);
    assert!(config_history_page_1.has_more);
    assert_eq!(config_history_page_1.data[0].contract_id, "venear.dao");
    assert_eq!(config_history_page_1.data[0].field, "guardians");
    assert_eq!(
        config_history_page_1.data[0].value,
        vec!["guardian4.near".to_string()]
    );
    assert_eq!(config_history_page_1.data[0].source, "set_method");

    let config_history_cursor = config_history_page_1
        .next_cursor
        .clone()
        .expect("config history cursor");

    let config_history_page_2: VenearConfigHistoryListResponse = get_json(
        &router,
        &format!("/api/v1/venear/config/history?limit=2&cursor={config_history_cursor}"),
    )
    .await;
    assert_eq!(config_history_page_2.data.len(), 2);
    assert!(config_history_page_2.has_more);
    assert_eq!(config_history_page_2.data[0].field, "guardians");
    assert_eq!(config_history_page_2.data[0].contract_id, "vote.dao");
    assert_eq!(config_history_page_2.data[0].source, "set_method");

    let reviewer_history: VenearConfigHistoryListResponse =
        get_json(&router, "/api/v1/venear/config/history?field=reviewer_ids").await;
    assert_eq!(reviewer_history.data.len(), 2);
    assert!(reviewer_history
        .data
        .iter()
        .all(|entry| entry.field == "reviewer_ids"));

    let invalid_field_response = router
        .clone()
        .into_service()
        .oneshot(
            Request::get("/api/v1/venear/config/history?field=owners")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .expect("invalid field response");
    assert_eq!(invalid_field_response.status(), StatusCode::BAD_REQUEST);

    let proposal: VenearProposalDto = get_json(&router, "/api/v1/venear/proposals/1").await;
    assert_eq!(proposal.proposal_id, 1);
    assert_eq!(proposal.status, "finished");
    assert_eq!(proposal.reviewer_id.as_deref(), Some("reviewer.near"));
    assert_eq!(
        proposal.latest_event.transaction_hash.as_deref(),
        Some("tx-p1-create")
    );
    assert_eq!(proposal.vote_aggregates.len(), 2);

    let proposal_events: VenearProposalEventsListResponse =
        get_json(&router, "/api/v1/venear/proposals/1/events").await;
    assert_eq!(proposal_events.data.len(), 2);
    assert_eq!(proposal_events.data[0].event_type, "proposal_approve");
    assert_eq!(
        proposal_events.data[0].transaction_hash.as_deref(),
        Some("tx-p1-final")
    );
    assert_eq!(
        proposal_events.data[1].transaction_hash.as_deref(),
        Some("tx-p1-create")
    );

    let finalized_events: VenearProposalEventsListResponse = get_json(
        &router,
        "/api/v1/venear/proposals/1/events?event_type=proposal_approve&actor_account_id=reviewer.near",
    )
    .await;
    assert_eq!(finalized_events.data.len(), 1);
    assert_eq!(finalized_events.data[0].event_type, "proposal_approve");

    let votes: VenearVotesListResponse =
        get_json(&router, "/api/v1/venear/proposals/1/votes").await;
    assert_eq!(votes.data.len(), 2);
    assert_eq!(votes.data[0].voter_account_id, "jane.near");
    assert_eq!(votes.data[0].voting_power, "900000000000000000000");
    assert_eq!(votes.data[0].transaction_hash.as_deref(), Some("tx-vote-2"));
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
    assert_eq!(voting_power.data[1].event_type, "ft_mint");
    assert_eq!(voting_power.data[1].reason, "balance_increase");
    assert_eq!(voting_power.data[2].event_type, "add_vote");
    assert_eq!(voting_power.data[2].proposal_id, Some(1));
    assert_eq!(voting_power.data[2].voting_power, "8");
    assert_eq!(voting_power.data[2].reason, "vote_cast");
    assert_eq!(
        voting_power.data[0].transaction_hash.as_deref(),
        Some("tx-deleg-1")
    );
    assert_eq!(
        voting_power.data[1].transaction_hash.as_deref(),
        Some("tx-vp-1")
    );
    assert_eq!(
        voting_power.data[2].transaction_hash.as_deref(),
        Some("tx-vp-2")
    );

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
    assert_eq!(jane_vp.data[0].event_type, "add_vote");
    assert_eq!(jane_vp.data[0].reason, "vote_cast");

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
    assert_eq!(
        delegations.data[0].transaction_hash.as_deref(),
        Some("tx-deleg-1")
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

    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body bytes");
    if status != StatusCode::OK {
        panic!(
            "request path: {path}, status: {status}, body: {}",
            String::from_utf8_lossy(&bytes)
        );
    }

    serde_json::from_slice(&bytes).expect("json body")
}

async fn seed_venear_data(db: &impl ConnectionTrait) -> anyhow::Result<()> {
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

async fn execute_sql(db: &impl ConnectionTrait, sql: &str) -> anyhow::Result<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        vec![],
    ))
    .await?;

    Ok(())
}
