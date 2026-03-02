use sea_orm::{ConnectionTrait, Database, DatabaseBackend, DbErr, Statement};
use std::env;
use std::process::ExitCode;

const BACKFILL_DISCOURSE_ACCOUNTS_SQL: &str = r#"
INSERT INTO identity.accounts (platform_id, scope, external_id)
SELECT
    'discourse',
    u.instance,
    u.external_id::text
FROM discourse.discourse_users u
ON CONFLICT (platform_id, scope, external_id) DO NOTHING
"#;

const BACKFILL_TELEGRAM_ACCOUNTS_SQL: &str = r#"
INSERT INTO identity.accounts (platform_id, scope, external_id)
SELECT
    'telegram',
    'global',
    u.telegram_id::text
FROM telegram.telegram_users u
ON CONFLICT (platform_id, scope, external_id) DO NOTHING
"#;

const ENSURE_IDENTITY_PLATFORMS_SQL: &str = r#"
INSERT INTO identity.platforms (id, label)
VALUES
    ('discourse', 'Discourse'),
    ('near', 'NEAR'),
    ('telegram', 'Telegram'),
    ('venear_governance', 'veNEAR Governance'),
    ('venear_lockup', 'veNEAR Lockup')
ON CONFLICT (id) DO UPDATE
SET label = EXCLUDED.label
"#;

const BACKFILL_VENEAR_GOVERNANCE_ACCOUNTS_PRIMARY_SQL: &str = r#"
INSERT INTO identity.accounts (platform_id, scope, external_id)
SELECT
    'venear_governance',
    $1,
    lower(a.account_id)
FROM near.near_accounts a
WHERE a.account_id IS NOT NULL
  AND btrim(a.account_id) <> ''
  AND lower(a.account_id) NOT LIKE '%.venear.dao'
ON CONFLICT (platform_id, scope, external_id) DO NOTHING
"#;

const BACKFILL_VENEAR_LOCKUP_ACCOUNTS_PRIMARY_SQL: &str = r#"
INSERT INTO identity.accounts (platform_id, scope, external_id)
SELECT
    'venear_lockup',
    $1,
    lower(a.account_id)
FROM near.near_accounts a
WHERE a.account_id IS NOT NULL
  AND btrim(a.account_id) <> ''
  AND lower(a.account_id) LIKE '%.venear.dao'
ON CONFLICT (platform_id, scope, external_id) DO NOTHING
"#;

const BACKFILL_VENEAR_GOVERNANCE_ACCOUNTS_FALLBACK_SQL: &str = r#"
WITH governance_accounts(account_id) AS (
    SELECT lower(v.account_id) FROM near.near_venear_account_balances v
    UNION
    SELECT lower(v.predecessor_account_id) FROM near.near_venear_account_balances v
    UNION
    SELECT lower(v.contract_id) FROM near.near_venear_account_balances v
    UNION
    SELECT lower(v.account_id) FROM near.near_venear_votes v
    UNION
    SELECT lower(v.predecessor_account_id) FROM near.near_venear_votes v
    UNION
    SELECT lower(v.contract_id) FROM near.near_venear_votes v
    UNION
    SELECT lower(v.account_id) FROM near.near_venear_proposal_events v
    UNION
    SELECT lower(v.proposer_id) FROM near.near_venear_proposal_events v
    UNION
    SELECT lower(v.reviewer_id) FROM near.near_venear_proposal_events v
    UNION
    SELECT lower(v.predecessor_account_id) FROM near.near_venear_proposal_events v
    UNION
    SELECT lower(v.contract_id) FROM near.near_venear_proposal_events v
    UNION
    SELECT lower(v.owner_account_id) FROM near.near_venear_lockups v
    UNION
    SELECT lower(v.owner_account_id) FROM near.near_venear_lockup_actions v
    UNION
    SELECT lower(v.predecessor_account_id) FROM near.near_venear_lockup_actions v
    UNION
    SELECT lower(v.receiver_account_id) FROM near.near_venear_lockup_actions v
    UNION
    SELECT lower(v.staking_pool_account_id) FROM near.near_venear_lockup_actions v
    UNION
    SELECT lower(v.predecessor_account_id) FROM near.near_venear_events_raw v
    UNION
    SELECT lower(v.contract_id) FROM near.near_venear_events_raw v
)
INSERT INTO identity.accounts (platform_id, scope, external_id)
SELECT
    'venear_governance',
    $1,
    account_id
FROM governance_accounts
WHERE account_id IS NOT NULL
  AND btrim(account_id) <> ''
  AND account_id NOT LIKE '%.venear.dao'
ON CONFLICT (platform_id, scope, external_id) DO NOTHING
"#;

const BACKFILL_VENEAR_LOCKUP_ACCOUNTS_FALLBACK_SQL: &str = r#"
WITH lockup_accounts(account_id) AS (
    SELECT lower(v.lockup_account_id) FROM near.near_venear_lockups v
    UNION
    SELECT lower(v.lockup_account_id) FROM near.near_venear_lockup_actions v
)
INSERT INTO identity.accounts (platform_id, scope, external_id)
SELECT
    'venear_lockup',
    $1,
    account_id
FROM lockup_accounts
WHERE account_id IS NOT NULL
  AND btrim(account_id) <> ''
ON CONFLICT (platform_id, scope, external_id) DO NOTHING
"#;

const CLEAR_CURRENT_DISCOURSE_ALIASES_SQL: &str = r#"
UPDATE identity.account_aliases aa
SET
    is_current = FALSE,
    updated_at = CURRENT_TIMESTAMP
FROM identity.accounts a
WHERE aa.account_id = a.id
  AND a.platform_id = 'discourse'
  AND aa.is_current = TRUE
"#;

const BACKFILL_DISCOURSE_ALIASES_SQL: &str = r#"
INSERT INTO identity.account_aliases (
    account_id,
    alias,
    alias_normalized,
    first_seen_at,
    last_seen_at,
    is_current
)
SELECT
    a.id,
    u.username,
    lower(u.username),
    CURRENT_TIMESTAMP,
    CURRENT_TIMESTAMP,
    TRUE
FROM discourse.discourse_users u
JOIN identity.accounts a
  ON a.platform_id = 'discourse'
 AND a.scope = u.instance
 AND a.external_id = u.external_id::text
WHERE u.username IS NOT NULL
  AND btrim(u.username) <> ''
ON CONFLICT (account_id, alias_normalized)
DO UPDATE SET
    alias = EXCLUDED.alias,
    last_seen_at = EXCLUDED.last_seen_at,
    is_current = TRUE,
    updated_at = CURRENT_TIMESTAMP
"#;

const CLEAR_CURRENT_TELEGRAM_ALIASES_SQL: &str = r#"
UPDATE identity.account_aliases aa
SET
    is_current = FALSE,
    updated_at = CURRENT_TIMESTAMP
FROM identity.accounts a
WHERE aa.account_id = a.id
  AND a.platform_id = 'telegram'
  AND aa.is_current = TRUE
"#;

const BACKFILL_TELEGRAM_ALIASES_SQL: &str = r#"
INSERT INTO identity.account_aliases (
    account_id,
    alias,
    alias_normalized,
    first_seen_at,
    last_seen_at,
    is_current
)
SELECT
    a.id,
    u.username,
    lower(u.username),
    u.created_at,
    u.updated_at,
    TRUE
FROM telegram.telegram_users u
JOIN identity.accounts a
  ON a.platform_id = 'telegram'
 AND a.scope = 'global'
 AND a.external_id = u.telegram_id::text
WHERE u.username IS NOT NULL
  AND btrim(u.username) <> ''
ON CONFLICT (account_id, alias_normalized)
DO UPDATE SET
    alias = EXCLUDED.alias,
    last_seen_at = EXCLUDED.last_seen_at,
    is_current = TRUE,
    updated_at = CURRENT_TIMESTAMP
"#;

#[async_std::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("identity accounts backfill failed: {err}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let database_url = env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL is not set. Export DATABASE_URL first.".to_string())?;
    let database_url = normalize_database_url(&database_url);
    let near_chain_scope = parse_near_chain_scope()?;

    let db = Database::connect(database_url)
        .await
        .map_err(|err| format!("failed to connect to database: {err}"))?;

    ensure_identity_schema_ready(&db)
        .await
        .map_err(|err| format!("identity schema check failed: {err}"))?;

    execute_sql(
        &db,
        "ensure identity platforms",
        ENSURE_IDENTITY_PLATFORMS_SQL,
    )
    .await?;

    execute_sql(
        &db,
        "backfill discourse accounts",
        BACKFILL_DISCOURSE_ACCOUNTS_SQL,
    )
    .await?;
    execute_sql(
        &db,
        "backfill telegram accounts",
        BACKFILL_TELEGRAM_ACCOUNTS_SQL,
    )
    .await?;
    execute_sql_with_chain(
        &db,
        "backfill venear governance accounts (near_accounts)",
        BACKFILL_VENEAR_GOVERNANCE_ACCOUNTS_PRIMARY_SQL,
        &near_chain_scope,
    )
    .await?;
    execute_sql_with_chain(
        &db,
        "backfill venear lockup accounts (near_accounts)",
        BACKFILL_VENEAR_LOCKUP_ACCOUNTS_PRIMARY_SQL,
        &near_chain_scope,
    )
    .await?;
    execute_sql_with_chain(
        &db,
        "backfill venear governance accounts (fallback)",
        BACKFILL_VENEAR_GOVERNANCE_ACCOUNTS_FALLBACK_SQL,
        &near_chain_scope,
    )
    .await?;
    execute_sql_with_chain(
        &db,
        "backfill venear lockup accounts (fallback)",
        BACKFILL_VENEAR_LOCKUP_ACCOUNTS_FALLBACK_SQL,
        &near_chain_scope,
    )
    .await?;

    execute_sql(
        &db,
        "clear current discourse aliases",
        CLEAR_CURRENT_DISCOURSE_ALIASES_SQL,
    )
    .await?;
    execute_sql(
        &db,
        "backfill discourse aliases",
        BACKFILL_DISCOURSE_ALIASES_SQL,
    )
    .await?;

    execute_sql(
        &db,
        "clear current telegram aliases",
        CLEAR_CURRENT_TELEGRAM_ALIASES_SQL,
    )
    .await?;
    execute_sql(
        &db,
        "backfill telegram aliases",
        BACKFILL_TELEGRAM_ALIASES_SQL,
    )
    .await?;

    print_account_summary(&db)
        .await
        .map_err(|err| format!("failed to print account summary: {err}"))?;

    Ok(())
}

fn normalize_database_url(url: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return format!("{url}?sslmode=require");
    };

    let uses_system_rootcert = query
        .split('&')
        .any(|entry| entry.eq_ignore_ascii_case("sslrootcert=system"));

    if !uses_system_rootcert {
        return url.to_string();
    }

    let mut rewritten: Vec<String> = query
        .split('&')
        .filter(|entry| !entry.to_ascii_lowercase().starts_with("sslrootcert="))
        .filter(|entry| !entry.to_ascii_lowercase().starts_with("sslmode="))
        .map(ToString::to_string)
        .collect();

    rewritten.push("sslmode=require".to_string());

    format!("{base}?{}", rewritten.join("&"))
}

fn parse_near_chain_scope() -> Result<String, String> {
    let scope = env::var("IDENTITY_NEAR_CHAIN").unwrap_or_else(|_| "mainnet".to_string());
    let scope = scope.trim().to_ascii_lowercase();

    if scope.is_empty() {
        return Err("IDENTITY_NEAR_CHAIN cannot be empty".to_string());
    }

    if scope
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        Ok(scope)
    } else {
        Err(format!(
            "IDENTITY_NEAR_CHAIN contains invalid characters: {scope}"
        ))
    }
}

async fn ensure_identity_schema_ready(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    let stmt = Statement::from_string(
        DatabaseBackend::Postgres,
        "
        SELECT EXISTS (
            SELECT 1
            FROM information_schema.schemata
            WHERE schema_name = 'identity'
        ) AS exists
        "
        .to_string(),
    );

    let row = db
        .query_one(stmt)
        .await?
        .ok_or_else(|| DbErr::Custom("schema existence query returned no rows".to_string()))?;

    let exists: bool = row.try_get("", "exists")?;
    if exists {
        Ok(())
    } else {
        Err(DbErr::Custom(
            "identity schema is missing; run `cargo migrate-up` first".to_string(),
        ))
    }
}

async fn execute_sql(db: &impl ConnectionTrait, label: &str, sql: &str) -> Result<u64, String> {
    let stmt = Statement::from_string(DatabaseBackend::Postgres, sql.to_string());
    let result = db
        .execute(stmt)
        .await
        .map_err(|err| format!("{label}: {err}"))?;
    let rows = result.rows_affected();
    println!("{label}: {rows} rows affected");
    Ok(rows)
}

async fn execute_sql_with_chain(
    db: &impl ConnectionTrait,
    label: &str,
    sql: &str,
    near_chain_scope: &str,
) -> Result<u64, String> {
    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        vec![near_chain_scope.to_string().into()],
    );
    let result = db
        .execute(stmt)
        .await
        .map_err(|err| format!("{label}: {err}"))?;
    let rows = result.rows_affected();
    println!("{label}: {rows} rows affected");
    Ok(rows)
}

async fn print_account_summary(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    let stmt = Statement::from_string(
        DatabaseBackend::Postgres,
        "
        SELECT platform_id, COUNT(*)::bigint AS count
        FROM identity.accounts
        GROUP BY platform_id
        ORDER BY platform_id
        "
        .to_string(),
    );

    let rows = db.query_all(stmt).await?;
    println!("identity.accounts summary:");
    for row in rows {
        let platform_id: String = row.try_get("", "platform_id")?;
        let count: i64 = row.try_get("", "count")?;
        println!("  - {platform_id}: {count}");
    }

    Ok(())
}
