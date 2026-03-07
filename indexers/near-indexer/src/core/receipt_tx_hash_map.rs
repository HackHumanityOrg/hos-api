use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::collections::{HashMap, HashSet};

pub(crate) const RECEIPT_TX_HASH_LOOKUP_BATCH_SIZE: usize = 2_048;

pub(crate) fn receipt_tx_hash_lookup_batch_count(receipt_id_count: usize) -> usize {
    if receipt_id_count == 0 {
        0
    } else {
        receipt_id_count.div_ceil(RECEIPT_TX_HASH_LOOKUP_BATCH_SIZE)
    }
}

/// Load persisted `receipt_id -> transaction_hash` mappings.
pub async fn load_receipt_tx_hash_map(
    db: &impl ConnectionTrait,
    receipt_ids: &[String],
) -> Result<HashMap<String, String>> {
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

    let mut result = HashMap::new();
    for chunk in ids.chunks(RECEIPT_TX_HASH_LOOKUP_BATCH_SIZE) {
        let mut placeholders = String::new();
        for idx in 0..chunk.len() {
            if idx > 0 {
                placeholders.push_str(", ");
            }
            let value_idx = idx + 1;
            placeholders.push_str(&format!("(${value_idx})"));
        }

        let sql = format!(
            "SELECT m.receipt_id, m.transaction_hash
             FROM near.near_receipt_tx_hash_map m
             JOIN (VALUES {placeholders}) AS lookup(receipt_id)
               ON lookup.receipt_id = m.receipt_id"
        );

        let rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &sql,
                chunk.iter().cloned().map(Into::into).collect::<Vec<_>>(),
            ))
            .await
            .context("failed to load near_receipt_tx_hash_map rows")?;

        for row in rows {
            let receipt_id: String = row
                .try_get("", "receipt_id")
                .context("failed to read near_receipt_tx_hash_map.receipt_id")?;
            let transaction_hash: String = row
                .try_get("", "transaction_hash")
                .context("failed to read near_receipt_tx_hash_map.transaction_hash")?;
            result.insert(receipt_id, transaction_hash);
        }
    }

    Ok(result)
}

/// Upsert a single receipt->tx mapping while preserving first-seen mapping.
/// Returns the persisted tx hash (which can differ from the input if a conflict exists).
pub async fn upsert_receipt_tx_hash(
    db: &impl ConnectionTrait,
    receipt_id: &str,
    transaction_hash: &str,
    block_height: i64,
) -> Result<String> {
    let row = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO near.near_receipt_tx_hash_map (
                receipt_id,
                transaction_hash,
                first_seen_height,
                last_seen_height,
                created_at,
                updated_at
            ) VALUES ($1, $2, $3, $3, NOW(), NOW())
            ON CONFLICT (receipt_id) DO UPDATE SET
                last_seen_height = GREATEST(
                    near.near_receipt_tx_hash_map.last_seen_height,
                    EXCLUDED.last_seen_height
                ),
                updated_at = NOW()
            RETURNING transaction_hash",
            vec![
                receipt_id.into(),
                transaction_hash.into(),
                block_height.into(),
            ],
        ))
        .await
        .context("failed to upsert near_receipt_tx_hash_map")?
        .context("upsert near_receipt_tx_hash_map returned no row")?;

    let persisted_tx_hash: String = row
        .try_get("", "transaction_hash")
        .context("failed to read persisted transaction_hash")?;

    Ok(persisted_tx_hash)
}
