use api::types::{
    AgoraDelegateDetailResponse, AgoraDelegateStatementChangesResponse,
    AgoraDelegateVotingHistoryResponse, AgoraDelegatesResponse, AgoraDelegationEventsResponse,
    AgoraHosActivityResponse, AgoraProposalChartsResponse, AgoraProposalListResponse,
    AgoraProposalNonVotersResponse, AgoraProposalQuorumResponse, AgoraProposalVotesResponse,
    AgoraVoteChangesResponse, AgoraVotingPowerChartPoint,
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
    Router,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use tower::ServiceExt;

#[path = "../support/mod.rs"]
mod support;

#[tokio::test]
async fn agora_endpoints_return_seeded_data() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;
    support::seed_venear_data(database.db().as_ref())
        .await
        .expect("seed agora data");

    let approved: AgoraProposalListResponse =
        get_json(&router, "/api/agora/proposal/approved").await;
    assert_eq!(approved.count, 1);
    assert_eq!(approved.proposals.len(), 1);
    assert_eq!(approved.proposals[0].id, "rcpt-p1-create");
    assert_eq!(approved.proposals[0].receipt_id, "rcpt-p1-create");
    assert_eq!(approved.proposals[0].proposal_id.as_deref(), Some("1"));
    assert_eq!(approved.proposals[0].proposal_type, "SimpleMajority");
    assert_eq!(approved.proposals[0].approval_threshold, "5000");
    assert_eq!(
        approved.proposals[0].quorum_amount,
        "10000000000000000000000000"
    );
    assert_eq!(approved.proposals[0].status, "Finished");

    let pending: AgoraProposalListResponse =
        get_json(&router, "/api/agora/proposal/pending?created_by=bob.near").await;
    assert_eq!(pending.count, 1);
    assert_eq!(pending.proposals.len(), 1);
    assert_eq!(pending.proposals[0].proposal_id.as_deref(), Some("2"));
    assert_eq!(pending.proposals[0].status, "Created");

    let votes: AgoraProposalVotesResponse = get_json(&router, "/api/agora/proposal/1/votes").await;
    assert_eq!(votes.count, 2);
    assert_eq!(votes.votes.len(), 2);
    assert_eq!(votes.votes[0].account_id, "jane.near");
    assert_eq!(votes.votes[0].voting_power, "900000000000000000000");
    assert_eq!(votes.votes[0].vote_option, "0");
    assert_eq!(votes.votes[1].account_id, "lane.near");
    assert_eq!(votes.votes[1].voting_power, "8");

    let charts: AgoraProposalChartsResponse =
        get_json(&router, "/api/agora/proposal/1/charts").await;
    assert_eq!(charts.data.len(), 2);
    assert_eq!(charts.data[0].account_id, "lane.near");
    assert_eq!(charts.data[0].vote_option, "1");
    assert_eq!(charts.data[1].account_id, "jane.near");
    assert_eq!(charts.data[1].vote_option, "0");

    let non_voters: AgoraProposalNonVotersResponse =
        get_json(&router, "/api/agora/proposal/2/non-voters").await;
    assert_eq!(non_voters.count, 2);
    assert_eq!(non_voters.non_voters.len(), 2);
    assert_eq!(non_voters.non_voters[0].registered_voter_id, "jane.near");
    assert_eq!(non_voters.non_voters[0].voting_power, "8");
    assert_eq!(non_voters.non_voters[1].registered_voter_id, "lane.near");
    assert_eq!(non_voters.non_voters[1].voting_power, "0");

    let quorum: AgoraProposalQuorumResponse =
        get_json(&router, "/api/agora/proposal/1/quorum").await;
    assert_eq!(quorum.quorum_amount, "10000000000000000000000000");

    let pending_quorum = get_status_json(&router, "/api/agora/proposal/2/quorum").await;
    assert_eq!(pending_quorum.0, StatusCode::BAD_REQUEST);
    assert_eq!(pending_quorum.1["error"], "Proposal is not approved");

    let delegates: AgoraDelegatesResponse =
        get_json(&router, "/api/agora/delegates?order_by=most_voting_power").await;
    assert_eq!(delegates.count, 2);
    assert_eq!(delegates.delegates.len(), 2);
    assert_eq!(delegates.delegates[0].address, "jane.near");
    assert_eq!(delegates.delegates[0].voting_power.as_deref(), Some("8"));
    assert_eq!(
        delegates.delegates[0].participation_rate.as_deref(),
        Some("0.1")
    );
    assert!(delegates.delegates[0].twitter.is_none());
    assert!(delegates.delegates[0].discord.is_none());
    assert!(delegates.delegates[0].warpcast.is_none());
    assert!(delegates.delegates[0].statement.is_none());
    assert!(delegates.delegates[0].top_issues.is_none());
    assert!(delegates.delegates[0].endorsed.is_none());
    assert!(delegates.delegates[0].notification_preferences.is_none());

    let endorsed_only: AgoraDelegatesResponse =
        get_json(&router, "/api/agora/delegates?filter_by=endorsed").await;
    assert_eq!(endorsed_only.count, 0);
    assert!(endorsed_only.delegates.is_empty());

    let jane_delegate: AgoraDelegateDetailResponse =
        get_json(&router, "/api/agora/delegates/jane.near").await;
    assert_eq!(jane_delegate.delegate.address, "jane.near");
    assert_eq!(jane_delegate.delegate.voting_power.as_deref(), Some("8"));
    assert_eq!(jane_delegate.delegate.for_count, 1);
    assert_eq!(jane_delegate.delegate.against_count, 0);
    assert_eq!(jane_delegate.delegate.abstain_count, 0);
    assert_eq!(jane_delegate.delegate.delegated_from_count, 0);
    assert_eq!(
        jane_delegate.delegate.participation_rate.as_deref(),
        Some("0.1")
    );
    assert!(jane_delegate.delegate.twitter.is_none());
    assert!(jane_delegate.delegate.top_issues.is_none());
    assert!(jane_delegate.delegate.notification_preferences.is_none());

    let jane_voting_history: AgoraDelegateVotingHistoryResponse =
        get_json(&router, "/api/agora/delegates/jane.near/voting-history").await;
    assert_eq!(jane_voting_history.count, 1);
    assert_eq!(jane_voting_history.votes.len(), 1);
    assert_eq!(
        jane_voting_history.votes[0].address.as_deref(),
        Some("jane.near")
    );
    assert_eq!(
        jane_voting_history.votes[0].proposal_id.as_deref(),
        Some("1")
    );
    assert_eq!(
        jane_voting_history.votes[0].vote_option.as_deref(),
        Some("0")
    );

    let delegated_from: AgoraDelegationEventsResponse =
        get_json(&router, "/api/agora/delegates/jane.near/delegated-from").await;
    assert_eq!(delegated_from.count, 0);
    assert!(delegated_from.events.is_empty());

    let delegated_to: AgoraDelegationEventsResponse =
        get_json(&router, "/api/agora/delegates/lane.near/delegated-to").await;
    assert_eq!(delegated_to.count, 0);
    assert!(delegated_to.events.is_empty());

    let hos_activity: AgoraHosActivityResponse =
        get_json(&router, "/api/agora/delegates/lane.near/hos-activity").await;
    assert_eq!(hos_activity.count, 1);
    assert_eq!(hos_activity.hos_activity.len(), 1);
    assert_eq!(hos_activity.hos_activity[0].receipt_id, "rcpt-deleg-1");
    assert_eq!(
        hos_activity.hos_activity[0].transaction_type,
        "outbound_delegation"
    );
    assert_eq!(
        hos_activity.hos_activity[0].near_amount.as_deref(),
        Some("3")
    );
    assert!(hos_activity.hos_activity[0].locked_near_balance.is_none());

    let statement_changes: AgoraDelegateStatementChangesResponse = get_json(
        &router,
        "/api/agora/delegate_statement_changes?page=2&offset=5",
    )
    .await;
    assert_eq!(statement_changes.page, 2);
    assert_eq!(statement_changes.offset, 5);
    assert!(statement_changes.changes.is_empty());
    assert_eq!(statement_changes.count, 0);

    let chart: Vec<AgoraVotingPowerChartPoint> =
        get_json(&router, "/api/agora/get_voting_power_chart/jane.near").await;
    assert_eq!(chart.len(), 1);
    assert_eq!(chart[0].block_number, 143);
    assert_eq!(chart[0].vp, "3");

    let empty_vote_changes: AgoraVoteChangesResponse =
        get_json(&router, "/api/agora/vote_changes/1").await;
    assert!(empty_vote_changes.vote_changes.is_empty());

    let missing_delegate = get_status_json(&router, "/api/agora/delegates/missing.near").await;
    assert_eq!(missing_delegate.0, StatusCode::NOT_FOUND);
    assert_eq!(
        missing_delegate.1["message"],
        "Address not found in registered voters and is not a valid NEAR account"
    );
}

#[tokio::test]
async fn agora_vote_changes_return_full_history() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;
    support::seed_venear_data(database.db().as_ref())
        .await
        .expect("seed agora data");

    support::execute_sql(
        database.db().as_ref(),
        r#"
        INSERT INTO near.near_receipt_actions (
            id, block_height, receipt_id, execution_outcome_id, transaction_hash, shard_id,
            signer_account_id, signer_public_key, gas_price, action_kind, action_json,
            predecessor_id, receiver_id, block_hash, chunk_hash, author, method_name,
            gas, deposit, args_base64, args_json, action_index, block_timestamp
        ) VALUES
            ('a-vote-jane-2', 112, 'rcpt-vote-3', 'exec-vote-3', 'tx-vote-3', 0, 'system.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"vote"}}'::jsonb, 'jane.near', 'vote.dao', 'bh-112', 'ch-112', 'author.near', 'vote', 0, '0', NULL, '{"proposal_id":"1","vote":"1"}'::jsonb, 0, 1760000120000000000),
            ('a-vote-jane-3', 113, 'rcpt-vote-4', 'exec-vote-4', 'tx-vote-4', 0, 'system.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"vote"}}'::jsonb, 'jane.near', 'vote.dao', 'bh-113', 'ch-113', 'author.near', 'vote', 0, '0', NULL, '{"proposal_id":"1","vote":"2"}'::jsonb, 0, 1760000130000000000)
        "#,
    )
    .await
    .expect("insert extra vote actions");

    support::execute_sql(
        database.db().as_ref(),
        r#"
        INSERT INTO near.near_execution_outcomes (
            id, block_height, receipt_id, outcome, raw_data, shard_id, transaction_hash,
            block_hash, chunk_hash, gas_burnt, gas_used, tokens_burnt, executor_account_id,
            status, outcome_receipt_ids, executed_in_block_hash, logs, results_json, block_timestamp
        ) VALUES
            ('exec-vote-3', 112, 'rcpt-vote-3', '{}'::jsonb, NULL, 0, 'tx-vote-3', 'bh-112', 'ch-112', 0, 0, 0, 'vote.dao', 'SuccessValue', '[]'::jsonb, 'bh-112', '["EVENT_JSON:{\"event\":\"add_vote\",\"data\":[{\"account_balance\":\"900000000000000000000\"}]}"]'::jsonb, NULL, 1760000120000000000),
            ('exec-vote-4', 113, 'rcpt-vote-4', '{}'::jsonb, NULL, 0, 'tx-vote-4', 'bh-113', 'ch-113', 0, 0, 0, 'vote.dao', 'SuccessValue', '[]'::jsonb, 'bh-113', '["EVENT_JSON:{\"event\":\"add_vote\",\"data\":[{\"account_balance\":\"900000000000000000000\"}]}"]'::jsonb, NULL, 1760000130000000000)
        "#,
    )
    .await
    .expect("insert extra vote outcomes");

    let vote_changes: AgoraVoteChangesResponse =
        get_json(&router, "/api/agora/vote_changes/1").await;
    assert_eq!(vote_changes.vote_changes.len(), 1);
    assert_eq!(vote_changes.vote_changes[0].account_id, "jane.near");
    assert_eq!(vote_changes.vote_changes[0].changes.len(), 3);
    assert_eq!(vote_changes.vote_changes[0].changes[0].vote_option, 0);
    assert_eq!(vote_changes.vote_changes[0].changes[1].vote_option, 1);
    assert_eq!(vote_changes.vote_changes[0].changes[2].vote_option, 2);
}

#[tokio::test]
async fn agora_delegation_history_returns_legacy_event_shape() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;
    support::seed_venear_data(database.db().as_ref())
        .await
        .expect("seed agora data");

    support::execute_sql(
        database.db().as_ref(),
        r#"
        INSERT INTO near.near_receipt_actions (
            id, block_height, receipt_id, execution_outcome_id, transaction_hash, shard_id,
            signer_account_id, signer_public_key, gas_price, action_kind, action_json,
            predecessor_id, receiver_id, block_hash, chunk_hash, author, method_name,
            gas, deposit, args_base64, args_json, action_index, block_timestamp
        ) VALUES
            ('a-delegate-erin', 200, 'rcpt-deleg-erin', 'exec-deleg-erin', 'tx-deleg-erin', 0, 'system.near', NULL, NULL, 'FunctionCall', '{"FunctionCall":{"method_name":"delegate_all"}}'::jsonb, 'erin.near', 'venear.dao', 'bh-200', 'ch-200', 'author.near', 'delegate_all', 0, '0', NULL, '{"receiver_id":"jane.near"}'::jsonb, 0, 1760000800000000000)
        "#,
    )
    .await
    .expect("insert delegation action");

    support::execute_sql(
        database.db().as_ref(),
        r#"
        INSERT INTO near.near_execution_outcomes (
            id, block_height, receipt_id, outcome, raw_data, shard_id, transaction_hash,
            block_hash, chunk_hash, gas_burnt, gas_used, tokens_burnt, executor_account_id,
            status, outcome_receipt_ids, executed_in_block_hash, logs, results_json, block_timestamp
        ) VALUES
            ('exec-deleg-erin', 200, 'rcpt-deleg-erin', '{}'::jsonb, NULL, 0, 'tx-deleg-erin', 'bh-200', 'ch-200', 0, 0, 0, 'venear.dao', 'SuccessValue', '[]'::jsonb, 'bh-200', '["EVENT_JSON:{\"event\":\"ft_mint\",\"data\":[{\"owner_id\":\"jane.near\",\"amount\":\"5\"}]}"]'::jsonb, NULL, 1760000800000000000)
        "#,
    )
    .await
    .expect("insert delegation outcome");

    support::execute_sql(
        database.db().as_ref(),
        "SELECT near.refresh_hos_materialized_views()",
    )
    .await
    .expect("refresh materialized views");

    let delegated_from: AgoraDelegationEventsResponse =
        get_json(&router, "/api/agora/delegates/jane.near/delegated-from").await;
    assert_eq!(delegated_from.count, 1);
    assert_eq!(delegated_from.events.len(), 1);
    assert_eq!(delegated_from.events[0].delegator_id, "erin.near");
    assert_eq!(
        delegated_from.events[0].delegatee_id.as_deref(),
        Some("jane.near")
    );
    assert_eq!(
        delegated_from.events[0].delegate_method.as_deref(),
        Some("delegate_all")
    );
    assert_eq!(
        delegated_from.events[0].delegate_event.as_deref(),
        Some("ft_mint")
    );
    assert_eq!(delegated_from.events[0].near_amount.as_deref(), Some("5"));
    assert_eq!(
        delegated_from.events[0].block_height.as_deref(),
        Some("200")
    );

    let delegated_to: AgoraDelegationEventsResponse =
        get_json(&router, "/api/agora/delegates/erin.near/delegated-to").await;
    assert_eq!(delegated_to.count, 1);
    assert_eq!(delegated_to.events.len(), 1);
    assert_eq!(delegated_to.events[0].delegator_id, "erin.near");
    assert_eq!(
        delegated_to.events[0].delegatee_id.as_deref(),
        Some("jane.near")
    );
    assert_eq!(
        delegated_to.events[0].delegate_event.as_deref(),
        Some("ft_mint")
    );
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

async fn get_status_json(router: &Router, path: &str) -> (StatusCode, Value) {
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

    (
        status,
        serde_json::from_slice(&bytes).expect("json error body"),
    )
}
