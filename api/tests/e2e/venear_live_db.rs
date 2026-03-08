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
use serde::de::DeserializeOwned;
use tower::ServiceExt;

#[path = "../support/mod.rs"]
mod support;

#[tokio::test]
async fn venear_endpoints_return_seeded_data() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;
    support::seed_venear_data(database.db().as_ref())
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
