use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::collections::{HashMap, HashSet};

pub const RELEVANCE_SCOPE_ALL: &str = "all";
pub const RELEVANCE_SCOPE_HOS: &str = "hos";

pub const RELEVANCE_REASON_ROOT: &str = "root";
pub const RELEVANCE_REASON_DESCENDANT: &str = "descendant";
pub const RELEVANCE_REASON_OBSERVED: &str = "observed";

pub async fn load_relevant_receipts_for_scope(
    db: &impl ConnectionTrait,
    scope: &str,
    receipt_ids: &[String],
) -> Result<HashMap<String, Option<String>>> {
    let mut seen = HashSet::new();
    let mut ids = Vec::new();
    for receipt_id in receipt_ids {
        if seen.insert(receipt_id.clone()) {
            ids.push(receipt_id.clone());
        }
    }

    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let mut placeholders = String::new();
    for idx in 0..ids.len() {
        if idx > 0 {
            placeholders.push_str(", ");
        }
        placeholders.push_str(&format!("${}", idx + 2));
    }

    let sql = format!(
        "SELECT receipt_id, transaction_hash
         FROM near.near_relevant_receipts
         WHERE scope = $1
           AND receipt_id IN ({placeholders})"
    );

    let mut values = Vec::with_capacity(ids.len() + 1);
    values.push(scope.to_string().into());
    values.extend(ids.into_iter().map(Into::into));

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            values,
        ))
        .await
        .context("failed to load relevant receipt ids")?;

    let mut result = HashMap::new();
    for row in rows {
        let receipt_id: String = row
            .try_get("", "receipt_id")
            .context("failed to read relevant receipt_id")?;
        let transaction_hash: Option<String> = row
            .try_get("", "transaction_hash")
            .context("failed to read relevant transaction_hash")?;
        result.insert(receipt_id, transaction_hash);
    }

    Ok(result)
}

#[allow(clippy::too_many_arguments)]
pub async fn upsert_relevant_receipt(
    db: &impl ConnectionTrait,
    scope: &str,
    receipt_id: &str,
    reason: &str,
    discovered_by_receipt_id: Option<&str>,
    transaction_hash: Option<&str>,
    discovered_at_block_height: i64,
    executed: bool,
) -> Result<()> {
    db.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO near.near_relevant_receipts (
            scope,
            receipt_id,
            reason,
            discovered_by_receipt_id,
            transaction_hash,
            discovered_at_block_height,
            executed,
            created_at,
            updated_at
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), NOW())
        ON CONFLICT (scope, receipt_id) DO UPDATE SET
            reason = CASE
                WHEN CASE near.near_relevant_receipts.reason
                        WHEN 'root' THEN 3
                        WHEN 'descendant' THEN 2
                        WHEN 'observed' THEN 1
                        ELSE 0
                     END
                     >=
                     CASE EXCLUDED.reason
                        WHEN 'root' THEN 3
                        WHEN 'descendant' THEN 2
                        WHEN 'observed' THEN 1
                        ELSE 0
                     END
                THEN near.near_relevant_receipts.reason
                ELSE EXCLUDED.reason
            END,
            discovered_by_receipt_id = COALESCE(
                near.near_relevant_receipts.discovered_by_receipt_id,
                EXCLUDED.discovered_by_receipt_id
            ),
            transaction_hash = COALESCE(
                near.near_relevant_receipts.transaction_hash,
                EXCLUDED.transaction_hash
            ),
            discovered_at_block_height = LEAST(
                near.near_relevant_receipts.discovered_at_block_height,
                EXCLUDED.discovered_at_block_height
            ),
            executed = near.near_relevant_receipts.executed OR EXCLUDED.executed,
            updated_at = NOW()",
        vec![
            scope.into(),
            receipt_id.into(),
            reason.into(),
            discovered_by_receipt_id.map(ToString::to_string).into(),
            transaction_hash.map(ToString::to_string).into(),
            discovered_at_block_height.into(),
            executed.into(),
        ],
    ))
    .await
    .context("failed to upsert near.near_relevant_receipts")?;

    Ok(())
}
