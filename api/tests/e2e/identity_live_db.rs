use api::types::{IdentityPersonAccountsResponse, IdentityPersonsListResponse};
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
async fn identity_endpoints_return_seeded_data() {
    support::init_test_tracing();

    let (router, database) = support::router_with_database().await;
    seed_identity_data(database.db().as_ref())
        .await
        .expect("seed identity data");

    let persons_page_1: IdentityPersonsListResponse =
        get_json(&router, "/api/v1/identity/persons?limit=1").await;
    assert_eq!(persons_page_1.data.len(), 1);
    assert!(persons_page_1.has_more);
    assert_eq!(persons_page_1.data[0].id, 1);
    assert_eq!(persons_page_1.data[0].name.as_deref(), Some("alice"));
    let cursor = persons_page_1.next_cursor.clone().expect("next cursor");

    let persons_page_2: IdentityPersonsListResponse =
        get_json(&router, &format!("/api/v1/identity/persons?cursor={cursor}")).await;
    assert_eq!(persons_page_2.data.len(), 2);
    assert!(!persons_page_2.has_more);
    assert_eq!(persons_page_2.data[0].id, 2);
    assert_eq!(persons_page_2.data[0].name.as_deref(), Some("bob"));
    assert_eq!(persons_page_2.data[1].id, 3);
    assert_eq!(persons_page_2.data[1].name.as_deref(), Some("carol"));

    let accounts: IdentityPersonAccountsResponse =
        get_json(&router, "/api/v1/identity/persons/1/accounts").await;
    assert_eq!(accounts.person_id, 1);
    assert_eq!(accounts.accounts.len(), 2);
    assert_eq!(accounts.accounts[0].platform_id, "near");
    assert_eq!(accounts.accounts[0].external_id, "alice.near");
    assert_eq!(accounts.accounts[0].current_aliases, vec!["alice"]);
    assert_eq!(accounts.accounts[1].platform_id, "telegram");
    assert_eq!(accounts.accounts[1].external_id, "100001");
    assert_eq!(accounts.accounts[1].current_aliases, vec!["alice_tg"]);

    let empty_accounts: IdentityPersonAccountsResponse =
        get_json(&router, "/api/v1/identity/persons/3/accounts").await;
    assert_eq!(empty_accounts.person_id, 3);
    assert!(empty_accounts.accounts.is_empty());
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

async fn seed_identity_data(db: &impl ConnectionTrait) -> anyhow::Result<()> {
    execute_sql(
        db,
        r#"
        INSERT INTO identity.persons (id, name, created_at, updated_at)
        VALUES
            (1, 'alice', NOW(), NOW()),
            (2, 'bob', NOW(), NOW()),
            (3, 'carol', NOW(), NOW())
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO identity.accounts (id, platform_id, scope, external_id, created_at, updated_at)
        VALUES
            (10, 'near', 'mainnet', 'alice.near', NOW(), NOW()),
            (11, 'telegram', 'global', '100001', NOW(), NOW()),
            (12, 'discourse', 'gov.near.org', '42', NOW(), NOW())
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO identity.person_accounts (person_id, account_id, created_at, updated_at)
        VALUES
            (1, 10, NOW(), NOW()),
            (1, 11, NOW(), NOW()),
            (2, 12, NOW(), NOW())
        "#,
    )
    .await?;

    execute_sql(
        db,
        r#"
        INSERT INTO identity.account_aliases (
            account_id,
            alias,
            alias_normalized,
            first_seen_at,
            last_seen_at,
            is_current,
            created_at,
            updated_at
        )
        VALUES
            (10, 'alice', 'alice', NOW(), NOW(), TRUE, NOW(), NOW()),
            (11, 'alice_tg', 'alice_tg', NOW(), NOW(), TRUE, NOW(), NOW()),
            (12, 'alice_discourse', 'alice_discourse', NOW(), NOW(), FALSE, NOW(), NOW())
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
