use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::collections::{HashMap, HashSet};

fn build_placeholders(count: usize, start_idx: usize) -> String {
    (0..count)
        .map(|idx| format!("${}", start_idx + idx))
        .collect::<Vec<_>>()
        .join(", ")
}

async fn load_receipt_tx_hashes_from_execution_outcomes(
    db: &impl ConnectionTrait,
    receipt_ids: &[String],
) -> Result<HashMap<String, String>> {
    if receipt_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let sql = format!(
        "SELECT receipt_id, transaction_hash
         FROM near.near_execution_outcomes
         WHERE receipt_id IN ({})
           AND transaction_hash IS NOT NULL",
        build_placeholders(receipt_ids.len(), 1)
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            receipt_ids
                .iter()
                .cloned()
                .map(Into::into)
                .collect::<Vec<_>>(),
        ))
        .await
        .context("failed loading transaction hashes from near_execution_outcomes")?;

    let mut resolved = HashMap::new();
    for row in rows {
        let receipt_id: String = row
            .try_get("", "receipt_id")
            .context("failed reading near_execution_outcomes.receipt_id")?;
        let transaction_hash: String = row
            .try_get("", "transaction_hash")
            .context("failed reading near_execution_outcomes.transaction_hash")?;
        resolved.insert(receipt_id, transaction_hash);
    }

    Ok(resolved)
}

async fn load_receipt_tx_hashes_from_receipt_actions(
    db: &impl ConnectionTrait,
    receipt_ids: &[String],
) -> Result<HashMap<String, String>> {
    if receipt_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let sql = format!(
        "SELECT receipt_id, transaction_hash
         FROM near.near_receipt_actions
         WHERE receipt_id IN ({})
           AND transaction_hash IS NOT NULL",
        build_placeholders(receipt_ids.len(), 1)
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            receipt_ids
                .iter()
                .cloned()
                .map(Into::into)
                .collect::<Vec<_>>(),
        ))
        .await
        .context("failed loading transaction hashes from near_receipt_actions")?;

    let mut resolved = HashMap::new();
    for row in rows {
        let receipt_id: String = row
            .try_get("", "receipt_id")
            .context("failed reading near_receipt_actions.receipt_id")?;
        let transaction_hash: String = row
            .try_get("", "transaction_hash")
            .context("failed reading near_receipt_actions.transaction_hash")?;
        resolved.insert(receipt_id, transaction_hash);
    }

    Ok(resolved)
}

async fn load_receipt_tx_hashes_from_receipt_edges(
    db: &impl ConnectionTrait,
    receipt_ids: &[String],
) -> Result<HashMap<String, String>> {
    if receipt_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let placeholders = build_placeholders(receipt_ids.len(), 1);
    let sql = format!(
        "SELECT child_receipt_id AS receipt_id, transaction_hash
         FROM near.near_receipt_edges
         WHERE child_receipt_id IN ({placeholders})
           AND transaction_hash IS NOT NULL
         UNION ALL
         SELECT parent_receipt_id AS receipt_id, transaction_hash
         FROM near.near_receipt_edges
         WHERE parent_receipt_id IN ({}) 
           AND transaction_hash IS NOT NULL",
        build_placeholders(receipt_ids.len(), receipt_ids.len() + 1)
    );

    let mut values = receipt_ids
        .iter()
        .cloned()
        .map(Into::into)
        .collect::<Vec<_>>();
    values.extend(receipt_ids.iter().cloned().map(Into::into));

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            values,
        ))
        .await
        .context("failed loading transaction hashes from near_receipt_edges")?;

    let mut resolved = HashMap::new();
    for row in rows {
        let receipt_id: String = row
            .try_get("", "receipt_id")
            .context("failed reading near_receipt_edges.receipt_id")?;
        let transaction_hash: String = row
            .try_get("", "transaction_hash")
            .context("failed reading near_receipt_edges.transaction_hash")?;
        resolved.entry(receipt_id).or_insert(transaction_hash);
    }

    Ok(resolved)
}

async fn load_receipt_tx_hashes_from_parent_outcomes(
    db: &impl ConnectionTrait,
    receipt_ids: &[String],
) -> Result<HashMap<String, String>> {
    if receipt_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let sql = format!(
        "SELECT re.child_receipt_id AS receipt_id, eo.transaction_hash
         FROM near.near_receipt_edges re
         INNER JOIN near.near_execution_outcomes eo
            ON eo.id = re.parent_execution_outcome_id
         WHERE re.child_receipt_id IN ({})
           AND eo.transaction_hash IS NOT NULL",
        build_placeholders(receipt_ids.len(), 1)
    );

    let rows = db
        .query_all(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            receipt_ids
                .iter()
                .cloned()
                .map(Into::into)
                .collect::<Vec<_>>(),
        ))
        .await
        .context("failed loading transaction hashes from parent execution outcomes")?;

    let mut resolved = HashMap::new();
    for row in rows {
        let receipt_id: String = row
            .try_get("", "receipt_id")
            .context("failed reading parent-outcome receipt_id")?;
        let transaction_hash: String = row
            .try_get("", "transaction_hash")
            .context("failed reading parent-outcome transaction_hash")?;
        resolved.entry(receipt_id).or_insert(transaction_hash);
    }

    Ok(resolved)
}

pub async fn resolve_receipt_tx_hashes_from_canonical_graph(
    db: &impl ConnectionTrait,
    receipt_ids: &[String],
) -> Result<HashMap<String, String>> {
    let mut unresolved = receipt_ids.to_vec();
    let mut resolved = HashMap::new();

    let apply_stage = |resolved: &mut HashMap<String, String>,
                       unresolved: &mut Vec<String>,
                       stage_rows: HashMap<String, String>| {
        for (receipt_id, tx_hash) in stage_rows {
            resolved.entry(receipt_id).or_insert(tx_hash);
        }

        if !resolved.is_empty() {
            let unresolved_set: HashSet<String> = unresolved.drain(..).collect();
            *unresolved = unresolved_set
                .into_iter()
                .filter(|receipt_id| !resolved.contains_key(receipt_id))
                .collect();
        }
    };

    if !unresolved.is_empty() {
        let stage_rows = load_receipt_tx_hashes_from_execution_outcomes(db, &unresolved).await?;
        apply_stage(&mut resolved, &mut unresolved, stage_rows);
    }
    if !unresolved.is_empty() {
        let stage_rows = load_receipt_tx_hashes_from_receipt_actions(db, &unresolved).await?;
        apply_stage(&mut resolved, &mut unresolved, stage_rows);
    }
    if !unresolved.is_empty() {
        let stage_rows = load_receipt_tx_hashes_from_receipt_edges(db, &unresolved).await?;
        apply_stage(&mut resolved, &mut unresolved, stage_rows);
    }
    if !unresolved.is_empty() {
        let stage_rows = load_receipt_tx_hashes_from_parent_outcomes(db, &unresolved).await?;
        apply_stage(&mut resolved, &mut unresolved, stage_rows);
    }

    Ok(resolved)
}
