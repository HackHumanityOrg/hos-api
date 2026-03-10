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
async fn venear_proposals_votes_and_pagination_come_from_canonical_tables() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;
    support::seed_venear_canonical_data(database.db().as_ref())
        .await
        .expect("seed canonical venear data");

    let page_1: Value = get_json(&router, "/api/v1/venear/proposals?limit=1").await;
    assert_eq!(page_1["data"].as_array().expect("proposal page").len(), 1);
    assert_eq!(page_1["data"][0]["proposal_id"], 2);
    assert_eq!(page_1["data"][0]["status"], "created");
    assert!(page_1["data"][0]["snapshot"].is_null());
    assert_eq!(page_1["has_more"], true);

    let cursor = page_1["next_cursor"]
        .as_str()
        .expect("proposal next cursor");
    let page_2: Value = get_json(
        &router,
        &format!("/api/v1/venear/proposals?cursor={cursor}"),
    )
    .await;
    assert_eq!(page_2["data"].as_array().expect("proposal page").len(), 1);
    assert_eq!(page_2["data"][0]["proposal_id"], 1);
    assert_eq!(
        page_2["data"][0]["latest_event_type"],
        "proposal_approve_finalized"
    );
    assert_eq!(page_2["data"][0]["status"], "finished");
    assert_eq!(page_2["has_more"], false);

    let detail: Value = get_json(&router, "/api/v1/venear/proposals/1").await;
    assert_eq!(
        detail["snapshot"]["total_venear"]["raw"],
        "1000000000000000000"
    );
    assert_eq!(detail["turnout_voter_count"], 2);
    assert_eq!(detail["tally"][1]["votes_count"], 2);
    assert_eq!(detail["tally"][1]["voting_power"]["raw"], "11");

    let unfinalized_detail: Value = get_json(&router, "/api/v1/venear/proposals/2").await;
    assert!(unfinalized_detail["snapshot"].is_null());

    let timeline: Value = get_json(&router, "/api/v1/venear/proposals/1/timeline").await;
    let timeline_types = timeline["data"]
        .as_array()
        .expect("timeline rows")
        .iter()
        .map(|row| row["event_type"].as_str().expect("event type"))
        .collect::<Vec<_>>();
    assert_eq!(
        timeline_types,
        vec![
            "proposal_approve_finalized",
            "proposal_approve_requested",
            "create_proposal"
        ]
    );

    let votes: Value = get_json(&router, "/api/v1/venear/proposals/1/votes").await;
    assert_eq!(votes["data"].as_array().expect("vote rows").len(), 2);
    assert_eq!(votes["data"][0]["voter_account_id"], "jane.near");
    assert_eq!(votes["data"][0]["vote_option"], 1);
    assert_eq!(votes["data"][0]["voting_power"]["raw"], "3");
    assert_eq!(votes["data"][1]["voter_account_id"], "lane.near");

    let history: Value = get_json(&router, "/api/v1/venear/proposals/1/vote_history").await;
    assert_eq!(
        history["data"].as_array().expect("vote history rows").len(),
        4
    );
    assert_eq!(history["data"][0]["event_type"], "add_vote");
    assert_eq!(history["data"][0]["effective_after_event"], true);
    assert_eq!(history["data"][1]["event_type"], "remove_vote");
    assert_eq!(history["data"][2]["voter_account_id"], "jane.near");
    assert_eq!(history["data"][3]["voter_account_id"], "lane.near");
}

#[tokio::test]
async fn venear_accounts_delegations_lockups_and_rewards_are_account_centric() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;
    support::seed_venear_canonical_data(database.db().as_ref())
        .await
        .expect("seed canonical venear data");

    let accounts: Value = get_json(&router, "/api/v1/venear/accounts?limit=5").await;
    assert_eq!(accounts["data"][0]["account_id"], "lane.near");
    assert_eq!(accounts["data"][0]["effective_voting_power"]["raw"], "5");
    assert_eq!(accounts["data"][1]["account_id"], "jane.near");
    assert_eq!(accounts["data"][1]["effective_voting_power"]["raw"], "4");

    let lane: Value = get_json(&router, "/api/v1/venear/accounts/lane.near").await;
    assert_eq!(lane["effective_voting_power"]["raw"], "5");
    assert_eq!(lane["delegation"]["delegated_to_account_id"], "jane.near");
    assert_eq!(lane["lockup"]["status"], "active");
    assert!(array_contains(&lane["roles"], "delegator"));
    assert!(array_contains(&lane["roles"], "lockup_owner"));
    assert!(array_contains(&lane["roles"], "voter"));

    let reviewer_only: Value = get_json(&router, "/api/v1/venear/accounts/reviewer9.near").await;
    assert!(reviewer_only["governance_state"].is_null());
    assert!(array_contains(&reviewer_only["roles"], "config_actor"));

    let reviewer_delegation_history: Value = get_json(
        &router,
        "/api/v1/venear/accounts/reviewer9.near/delegation_history",
    )
    .await;
    assert_eq!(
        reviewer_delegation_history["data"]
            .as_array()
            .expect("empty delegation history")
            .len(),
        0
    );

    let reviewer_lockup_events: Value = get_json(
        &router,
        "/api/v1/venear/accounts/reviewer9.near/lockup_events",
    )
    .await;
    assert_eq!(
        reviewer_lockup_events["data"]
            .as_array()
            .expect("empty lockup events")
            .len(),
        0
    );

    let jane_votes: Value = get_json(&router, "/api/v1/venear/accounts/jane.near/votes").await;
    assert_eq!(
        jane_votes["data"].as_array().expect("account votes").len(),
        1
    );
    assert_eq!(jane_votes["data"][0]["proposal_id"], 1);
    assert_eq!(jane_votes["data"][0]["vote_option"], 1);

    let jane_vote_history: Value =
        get_json(&router, "/api/v1/venear/accounts/jane.near/vote_history").await;
    assert_eq!(
        jane_vote_history["data"]
            .as_array()
            .expect("account vote history")
            .len(),
        3
    );

    let jane_power_history: Value = get_json(
        &router,
        "/api/v1/venear/accounts/jane.near/voting_power_history",
    )
    .await;
    let reasons = jane_power_history["data"]
        .as_array()
        .expect("voting power history")
        .iter()
        .map(|row| row["reason"].as_str().expect("reason"))
        .collect::<Vec<_>>();
    assert!(reasons.contains(&"delegation_received"));
    assert!(reasons.contains(&"reward_claimed"));

    let lane_delegation_history: Value = get_json(
        &router,
        "/api/v1/venear/accounts/lane.near/delegation_history",
    )
    .await;
    assert_eq!(
        lane_delegation_history["data"]
            .as_array()
            .expect("delegation history")
            .len(),
        1
    );
    assert_eq!(lane_delegation_history["data"][0]["direction"], "outbound");
    assert_eq!(
        lane_delegation_history["data"][0]["counterparty_account_id"],
        "jane.near"
    );

    let lane_lockup_events: Value =
        get_json(&router, "/api/v1/venear/accounts/lane.near/lockup_events").await;
    assert_eq!(
        lane_lockup_events["data"]
            .as_array()
            .expect("lockup events")
            .len(),
        2
    );
    assert_eq!(lane_lockup_events["data"][0]["event_type"], "lockup_update");
    assert_eq!(
        lane_lockup_events["data"][1]["event_type"],
        "lockup_deployed"
    );

    let jane_rewards: Value =
        get_json(&router, "/api/v1/venear/accounts/jane.near/rewards_events").await;
    assert_eq!(
        jane_rewards["data"].as_array().expect("reward rows").len(),
        1
    );
    assert_eq!(jane_rewards["data"][0]["event_type"], "claim_rewards");
    assert_eq!(jane_rewards["data"][0]["amount"]["raw"], "1");
}

#[tokio::test]
async fn venear_config_admin_activity_and_lockups_expose_canonical_state() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;
    support::seed_venear_canonical_data(database.db().as_ref())
        .await
        .expect("seed canonical venear data");

    let config: Value = get_json(&router, "/api/v1/venear/config").await;
    let snapshots = config["data"].as_array().expect("config snapshots");
    assert_eq!(snapshots.len(), 2);

    let vote_snapshot = snapshots
        .iter()
        .find(|row| row["contract_id"] == "vote.dao")
        .expect("vote.dao config snapshot");
    assert_eq!(vote_snapshot["guardians"][0], "guardian2.near");
    assert_eq!(vote_snapshot["guardians"][1], "guardian3.near");
    assert_eq!(vote_snapshot["reviewer_ids"][0], "reviewer9.near");

    let reviewer_history: Value =
        get_json(&router, "/api/v1/venear/config/history?field=reviewer_ids").await;
    let reviewer_rows = reviewer_history["data"]
        .as_array()
        .expect("config history rows");
    assert_eq!(reviewer_rows.len(), 2);
    assert!(reviewer_rows
        .iter()
        .all(|row| row["field"] == "reviewer_ids"));

    let delegations: Value = get_json(&router, "/api/v1/venear/delegations").await;
    assert_eq!(
        delegations["data"].as_array().expect("delegations").len(),
        1
    );
    assert_eq!(delegations["data"][0]["delegator_account_id"], "lane.near");
    assert_eq!(delegations["data"][0]["delegatee_account_id"], "jane.near");
    assert_eq!(
        delegations["data"][0]["last_observed_delegated_voting_power"]["raw"],
        "3"
    );

    let lockups: Value = get_json(&router, "/api/v1/venear/lockups").await;
    let lockup_rows = lockups["data"].as_array().expect("lockup rows");
    assert_eq!(lockup_rows.len(), 2);
    let lane_lockup = lockup_rows
        .iter()
        .find(|row| row["owner_account_id"] == "lane.near")
        .expect("lane lockup");
    assert_eq!(lane_lockup["status"], "active");
    assert_eq!(lane_lockup["delegated_to_account_id"], "jane.near");
    let lane_lockup_id = lane_lockup["lockup_account_id"]
        .as_str()
        .expect("lane lockup id");

    let lockup_detail: Value =
        get_json(&router, &format!("/api/v1/venear/lockups/{lane_lockup_id}")).await;
    assert_eq!(lockup_detail["owner_account_id"], "lane.near");
    assert_eq!(lockup_detail["status"], "active");

    let lockup_events: Value = get_json(
        &router,
        &format!("/api/v1/venear/lockups/{lane_lockup_id}/events"),
    )
    .await;
    assert_eq!(
        lockup_events["data"]
            .as_array()
            .expect("lockup events")
            .len(),
        2
    );

    let failed_admin: Value = get_json(&router, "/api/v1/venear/admin/actions?success=false").await;
    assert_eq!(
        failed_admin["data"]
            .as_array()
            .expect("admin actions")
            .len(),
        1
    );
    assert_eq!(failed_admin["data"][0]["event_type"], "pause");
    assert_eq!(failed_admin["data"][0]["success"], false);

    let activity: Value = get_json(&router, "/api/v1/venear/activity").await;
    let kinds = activity["data"]
        .as_array()
        .expect("activity rows")
        .iter()
        .map(|row| row["kind"].as_str().expect("activity kind"))
        .collect::<Vec<_>>();
    for expected in [
        "proposal",
        "vote",
        "delegation",
        "lockup",
        "reward",
        "admin",
        "config",
    ] {
        assert!(
            kinds.contains(&expected),
            "missing activity kind {expected} from {kinds:?}"
        );
    }

    let reward_activity: Value = get_json(&router, "/api/v1/venear/activity?kind=reward").await;
    assert_eq!(
        reward_activity["data"]
            .as_array()
            .expect("reward activity")
            .len(),
        1
    );
    assert_eq!(reward_activity["data"][0]["event_type"], "claim_rewards");

    let failed_activity: Value = get_json(&router, "/api/v1/venear/activity?success=false").await;
    assert_eq!(
        failed_activity["data"]
            .as_array()
            .expect("failed activity")
            .len(),
        1
    );
    assert_eq!(failed_activity["data"][0]["kind"], "admin");
}

#[tokio::test]
async fn venear_missing_resources_and_removed_routes_return_not_found() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;
    support::seed_venear_canonical_data(database.db().as_ref())
        .await
        .expect("seed canonical venear data");

    let missing_proposal = get_status_json(&router, "/api/v1/venear/proposals/999").await;
    assert_eq!(missing_proposal.0, StatusCode::NOT_FOUND);

    let missing_account = get_status_json(&router, "/api/v1/venear/accounts/missing.near").await;
    assert_eq!(missing_account.0, StatusCode::NOT_FOUND);

    let missing_lockup = get_status_json(&router, "/api/v1/venear/lockups/missing.lockup").await;
    assert_eq!(missing_lockup.0, StatusCode::NOT_FOUND);

    let removed_delegates = get_status_json(&router, "/api/v1/venear/delegates").await;
    assert_eq!(removed_delegates.0, StatusCode::NOT_FOUND);

    let removed_config_current = get_status_json(&router, "/api/v1/venear/config/current").await;
    assert_eq!(removed_config_current.0, StatusCode::NOT_FOUND);
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
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("json body")
    };
    (status, body)
}

fn array_contains(value: &Value, needle: &str) -> bool {
    value
        .as_array()
        .expect("string array")
        .iter()
        .any(|entry| entry.as_str() == Some(needle))
}
