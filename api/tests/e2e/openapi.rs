use api::ApiDoc;
use utoipa::OpenApi;

#[test]
fn openapi_spec_exposes_health_path_and_schemas() {
    let doc = ApiDoc::openapi();
    let value = serde_json::to_value(&doc).expect("serialize openapi");

    let paths = value
        .get("paths")
        .expect("paths section")
        .as_object()
        .expect("paths object");
    assert!(paths.contains_key("/health"), "health path present");
    assert!(
        paths.contains_key("/api/v1/discourse/instances"),
        "discourse instances path present"
    );
    assert!(
        paths.contains_key("/api/v1/discourse/instances/{instance}/sync-lag"),
        "sync lag path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/proposals"),
        "venear proposals path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/proposals/{proposal_id}/events"),
        "venear proposal events path present"
    );
    assert!(
        paths.contains_key("/api/v1/venear/lockups/{lockup_account_id}/actions"),
        "venear lockup actions path present"
    );

    let schemas = value
        .get("components")
        .and_then(|components| components.get("schemas"))
        .and_then(|schemas| schemas.as_object())
        .expect("schemas listing");
    assert!(schemas.contains_key("HealthResponse"));
    assert!(schemas.contains_key("DatabaseHealth"));
    assert!(schemas.contains_key("ApiError"));
    assert!(schemas.contains_key("DiscourseTopicDto"));
    assert!(schemas.contains_key("SyncLagResponse"));
    assert!(schemas.contains_key("VenearProposalDto"));
    assert!(schemas.contains_key("VenearVoteDto"));
    assert!(schemas.contains_key("VenearLockupActionDto"));
}
