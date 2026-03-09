use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseBackend, QueryResult, Statement, Value};
use std::collections::HashMap;
use tracing::error;

use crate::core::canonical_rows::{
    NearBlockRow, NearChunkRow, NearExecutionOutcomeRow, NearReceiptActionRow, NearReceiptEdgeRow,
    NearReceiptRow, NearStateChangesRow, NearTransactionRow, ReceiptTxHashMapRow,
};

const MAX_BULK_INSERT_PARAMS: usize = 65_535;

#[derive(Clone, Copy)]
pub struct BulkInsertPlan {
    pub columns_per_row: usize,
    pub max_rows_per_statement: usize,
}

impl BulkInsertPlan {
    pub const fn new(columns_per_row: usize, max_rows_per_statement: usize) -> Self {
        Self {
            columns_per_row,
            max_rows_per_statement,
        }
    }
}

#[derive(Clone, Copy)]
pub enum NearTransactionWriteMode {
    InsertOnly,
    Upsert,
}

#[derive(Clone, Copy)]
pub enum NearReceiptWriteMode {
    InsertOnly,
    Upsert,
}

pub const RECEIPT_TX_MAP_BULK_INSERT_PLAN: BulkInsertPlan = BulkInsertPlan::new(3, 1_024);
pub const CANONICAL_TRANSACTION_BULK_INSERT_PLAN: BulkInsertPlan = BulkInsertPlan::new(11, 512);
pub const CANONICAL_RECEIPT_BULK_INSERT_PLAN: BulkInsertPlan = BulkInsertPlan::new(7, 512);
pub const CANONICAL_EXECUTION_OUTCOME_BULK_INSERT_PLAN: BulkInsertPlan =
    BulkInsertPlan::new(18, 384);
pub const CANONICAL_RECEIPT_ACTION_BULK_INSERT_PLAN: BulkInsertPlan = BulkInsertPlan::new(23, 256);
pub const CANONICAL_RECEIPT_EDGE_BULK_INSERT_PLAN: BulkInsertPlan = BulkInsertPlan::new(7, 512);
pub const CANONICAL_BLOCK_BULK_INSERT_PLAN: BulkInsertPlan = BulkInsertPlan::new(16, 256);
pub const CANONICAL_CHUNK_BULK_INSERT_PLAN: BulkInsertPlan = BulkInsertPlan::new(10, 384);
pub const CANONICAL_STATE_CHANGE_BULK_INSERT_PLAN: BulkInsertPlan = BulkInsertPlan::new(3, 512);

pub fn max_bulk_rows(plan: BulkInsertPlan) -> usize {
    let max_by_params = (MAX_BULK_INSERT_PARAMS / plan.columns_per_row.max(1)).max(1);
    max_by_params.min(plan.max_rows_per_statement.max(1))
}

pub fn batch_count(row_count: usize, plan: BulkInsertPlan) -> usize {
    if row_count == 0 {
        0
    } else {
        row_count.div_ceil(max_bulk_rows(plan))
    }
}

fn build_values_clause(row_count: usize, columns_per_row: usize) -> String {
    let mut sql = String::new();
    let mut placeholder = 1;
    for row_idx in 0..row_count {
        if row_idx > 0 {
            sql.push_str(", ");
        }
        sql.push('(');
        for col_idx in 0..columns_per_row {
            if col_idx > 0 {
                sql.push_str(", ");
            }
            sql.push('$');
            sql.push_str(&placeholder.to_string());
            placeholder += 1;
        }
        sql.push(')');
    }
    sql
}

async fn execute_chunked_upsert<T, F>(
    db: &impl ConnectionTrait,
    rows: &[T],
    plan: BulkInsertPlan,
    sql_prefix: &str,
    sql_suffix: &str,
    mut bind_row: F,
    context: &str,
) -> Result<()>
where
    F: FnMut(&T, &mut Vec<Value>),
{
    if rows.is_empty() {
        return Ok(());
    }

    let batch_size = max_bulk_rows(plan);
    for chunk in rows.chunks(batch_size) {
        let mut sql = String::from(sql_prefix);
        sql.push_str(&build_values_clause(chunk.len(), plan.columns_per_row));
        sql.push_str(sql_suffix);

        let mut values = Vec::with_capacity(chunk.len() * plan.columns_per_row);
        for row in chunk {
            bind_row(row, &mut values);
        }

        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sql,
            values,
        ))
        .await
        .with_context(|| context.to_string())?;
    }

    Ok(())
}

async fn query_chunked<T, F>(
    db: &impl ConnectionTrait,
    rows: &[T],
    plan: BulkInsertPlan,
    sql_prefix: &str,
    sql_suffix: &str,
    mut bind_row: F,
    context: &str,
) -> Result<Vec<QueryResult>>
where
    F: FnMut(&T, &mut Vec<Value>),
{
    if rows.is_empty() {
        return Ok(Vec::new());
    }

    let batch_size = max_bulk_rows(plan);
    let mut results = Vec::new();
    for chunk in rows.chunks(batch_size) {
        let mut sql = String::from(sql_prefix);
        sql.push_str(&build_values_clause(chunk.len(), plan.columns_per_row));
        sql.push_str(sql_suffix);

        let mut values = Vec::with_capacity(chunk.len() * plan.columns_per_row);
        for row in chunk {
            bind_row(row, &mut values);
        }

        let mut chunk_rows = db
            .query_all(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &sql,
                values,
            ))
            .await
            .with_context(|| context.to_string())?;
        results.append(&mut chunk_rows);
    }

    Ok(results)
}

pub async fn flush_receipt_tx_hash_map_upserts(
    db: &impl ConnectionTrait,
    rows: &[ReceiptTxHashMapRow],
    module_name: &str,
) -> Result<Vec<ReceiptTxHashMapRow>> {
    let query_rows = query_chunked(
        db,
        rows,
        RECEIPT_TX_MAP_BULK_INSERT_PLAN,
        "INSERT INTO near.near_receipt_tx_hash_map (
            receipt_id,
            transaction_hash,
            first_seen_height,
            last_seen_height,
            created_at,
            updated_at
        )
        SELECT
            incoming.receipt_id,
            incoming.transaction_hash,
            incoming.block_height,
            incoming.block_height,
            NOW(),
            NOW()
        FROM (VALUES ",
        "
        ) AS incoming(receipt_id, transaction_hash, block_height)
        ON CONFLICT (receipt_id) DO UPDATE SET
            last_seen_height = GREATEST(
                near.near_receipt_tx_hash_map.last_seen_height,
                EXCLUDED.last_seen_height
            ),
            updated_at = NOW()
        RETURNING receipt_id, transaction_hash",
        |row, values| {
            values.push(row.receipt_id.clone().into());
            values.push(row.transaction_hash.clone().into());
            values.push(row.block_height.into());
        },
        "failed to bulk upsert near_receipt_tx_hash_map",
    )
    .await?;

    let mut persisted = HashMap::new();
    let mut persisted_rows = Vec::new();
    for row in query_rows {
        let receipt_id: String = row
            .try_get("", "receipt_id")
            .context("failed to read bulk near_receipt_tx_hash_map.receipt_id")?;
        let transaction_hash: String = row
            .try_get("", "transaction_hash")
            .context("failed to read bulk near_receipt_tx_hash_map.transaction_hash")?;
        persisted_rows.push(ReceiptTxHashMapRow {
            receipt_id: receipt_id.clone(),
            transaction_hash: transaction_hash.clone(),
            block_height: 0,
        });
        persisted.insert(receipt_id, transaction_hash);
    }

    for row in rows {
        if let Some(persisted_tx_hash) = persisted.get(&row.receipt_id) {
            if persisted_tx_hash != &row.transaction_hash {
                error!(
                    module = module_name,
                    receipt_id = %row.receipt_id,
                    incoming_transaction_hash = %row.transaction_hash,
                    persisted_transaction_hash = %persisted_tx_hash,
                    "receipt->tx hash conflict detected; preserving first-seen mapping"
                );
            }
        }
    }

    Ok(persisted_rows)
}

pub async fn flush_near_blocks(db: &impl ConnectionTrait, rows: &[NearBlockRow]) -> Result<()> {
    execute_chunked_upsert(
        db,
        rows,
        CANONICAL_BLOCK_BULK_INSERT_PLAN,
        "INSERT INTO near.near_blocks (
            height,
            hash,
            prev_hash,
            epoch_id,
            next_epoch_id,
            author,
            timestamp,
            timestamp_nanosec,
            gas_price,
            total_supply,
            validator_reward,
            protocol_version,
            chunks_included,
            block_ordinal,
            prev_height,
            raw_data
        ) VALUES ",
        "
        ON CONFLICT (height) DO NOTHING",
        |row, values| {
            values.push(row.height.into());
            values.push(row.hash.clone().into());
            values.push(row.prev_hash.clone().into());
            values.push(row.epoch_id.clone().into());
            values.push(row.next_epoch_id.clone().into());
            values.push(row.author.clone().into());
            values.push(row.timestamp.into());
            values.push(row.timestamp_nanosec.into());
            values.push(row.gas_price.clone().into());
            values.push(row.total_supply.clone().into());
            values.push(row.validator_reward.clone().into());
            values.push(row.protocol_version.into());
            values.push(row.chunks_included.into());
            values.push(row.block_ordinal.into());
            values.push(row.prev_height.into());
            values.push(row.raw_data.clone().into());
        },
        "failed to bulk insert near_blocks",
    )
    .await
}

pub async fn flush_near_chunks(db: &impl ConnectionTrait, rows: &[NearChunkRow]) -> Result<()> {
    execute_chunked_upsert(
        db,
        rows,
        CANONICAL_CHUNK_BULK_INSERT_PLAN,
        "INSERT INTO near.near_chunks (
            chunk_hash,
            block_height,
            shard_id,
            author,
            height_created,
            height_included,
            gas_used,
            gas_limit,
            validator_reward,
            raw_data
        ) VALUES ",
        "
        ON CONFLICT (chunk_hash) DO NOTHING",
        |row, values| {
            values.push(row.chunk_hash.clone().into());
            values.push(row.block_height.into());
            values.push(row.shard_id.into());
            values.push(row.author.clone().into());
            values.push(row.height_created.into());
            values.push(row.height_included.into());
            values.push(row.gas_used.into());
            values.push(row.gas_limit.into());
            values.push(row.validator_reward.clone().into());
            values.push(row.raw_data.clone().into());
        },
        "failed to bulk insert near_chunks",
    )
    .await
}

pub async fn flush_near_transactions(
    db: &impl ConnectionTrait,
    rows: &[NearTransactionRow],
    mode: NearTransactionWriteMode,
) -> Result<()> {
    let sql_suffix = match mode {
        NearTransactionWriteMode::InsertOnly => {
            "
            ON CONFLICT (hash) DO NOTHING"
        }
        NearTransactionWriteMode::Upsert => {
            "
            ON CONFLICT (hash) DO UPDATE SET
                block_height = EXCLUDED.block_height,
                signer_id = EXCLUDED.signer_id,
                receiver_id = EXCLUDED.receiver_id,
                nonce = EXCLUDED.nonce,
                priority_fee = EXCLUDED.priority_fee,
                public_key = EXCLUDED.public_key,
                signature = EXCLUDED.signature,
                actions = EXCLUDED.actions,
                raw_data = COALESCE(EXCLUDED.raw_data, near.near_transactions.raw_data),
                shard_id = EXCLUDED.shard_id"
        }
    };

    execute_chunked_upsert(
        db,
        rows,
        CANONICAL_TRANSACTION_BULK_INSERT_PLAN,
        "INSERT INTO near.near_transactions (
            hash,
            block_height,
            signer_id,
            receiver_id,
            nonce,
            priority_fee,
            public_key,
            signature,
            actions,
            raw_data,
            shard_id
        ) VALUES ",
        sql_suffix,
        |row, values| {
            values.push(row.hash.clone().into());
            values.push(row.block_height.into());
            values.push(row.signer_id.clone().into());
            values.push(row.receiver_id.clone().into());
            values.push(row.nonce.into());
            values.push(row.priority_fee.into());
            values.push(row.public_key.clone().into());
            values.push(row.signature.clone().into());
            values.push(row.actions.clone().into());
            values.push(row.raw_data.clone().into());
            values.push(row.shard_id.into());
        },
        "failed to bulk write near_transactions",
    )
    .await
}

pub async fn flush_near_receipts(
    db: &impl ConnectionTrait,
    rows: &[NearReceiptRow],
    mode: NearReceiptWriteMode,
) -> Result<()> {
    let sql_suffix = match mode {
        NearReceiptWriteMode::InsertOnly => {
            "
            ON CONFLICT (receipt_id) DO NOTHING"
        }
        NearReceiptWriteMode::Upsert => {
            "
            ON CONFLICT (receipt_id) DO UPDATE SET
                block_height = EXCLUDED.block_height,
                shard_id = EXCLUDED.shard_id,
                predecessor_id = EXCLUDED.predecessor_id,
                receiver_id = EXCLUDED.receiver_id,
                receipt = EXCLUDED.receipt,
                raw_data = COALESCE(near.near_receipts.raw_data, EXCLUDED.raw_data)"
        }
    };

    execute_chunked_upsert(
        db,
        rows,
        CANONICAL_RECEIPT_BULK_INSERT_PLAN,
        "INSERT INTO near.near_receipts (
            receipt_id,
            block_height,
            shard_id,
            predecessor_id,
            receiver_id,
            receipt,
            raw_data
        ) VALUES ",
        sql_suffix,
        |row, values| {
            values.push(row.receipt_id.clone().into());
            values.push(row.block_height.into());
            values.push(row.shard_id.into());
            values.push(row.predecessor_id.clone().into());
            values.push(row.receiver_id.clone().into());
            values.push(row.receipt.clone().into());
            values.push(row.raw_data.clone().into());
        },
        "failed to bulk write near_receipts",
    )
    .await
}

pub async fn flush_near_execution_outcomes(
    db: &impl ConnectionTrait,
    rows: &[NearExecutionOutcomeRow],
) -> Result<()> {
    execute_chunked_upsert(
        db,
        rows,
        CANONICAL_EXECUTION_OUTCOME_BULK_INSERT_PLAN,
        "INSERT INTO near.near_execution_outcomes (
            id,
            block_height,
            shard_id,
            receipt_id,
            outcome,
            raw_data,
            transaction_hash,
            block_hash,
            chunk_hash,
            gas_burnt,
            tokens_burnt_yocto,
            executor_account_id,
            status,
            outcome_receipt_ids,
            executed_in_block_hash,
            logs,
            results_json,
            block_timestamp
        ) VALUES ",
        "
        ON CONFLICT (id) DO UPDATE SET
            block_height = EXCLUDED.block_height,
            shard_id = EXCLUDED.shard_id,
            receipt_id = COALESCE(near.near_execution_outcomes.receipt_id, EXCLUDED.receipt_id),
            outcome = EXCLUDED.outcome,
            raw_data = COALESCE(near.near_execution_outcomes.raw_data, EXCLUDED.raw_data),
            transaction_hash = COALESCE(near.near_execution_outcomes.transaction_hash, EXCLUDED.transaction_hash),
            block_hash = EXCLUDED.block_hash,
            chunk_hash = EXCLUDED.chunk_hash,
            gas_burnt = EXCLUDED.gas_burnt,
            tokens_burnt_yocto = EXCLUDED.tokens_burnt_yocto,
            executor_account_id = EXCLUDED.executor_account_id,
            status = EXCLUDED.status,
            outcome_receipt_ids = EXCLUDED.outcome_receipt_ids,
            executed_in_block_hash = EXCLUDED.executed_in_block_hash,
            logs = EXCLUDED.logs,
            results_json = EXCLUDED.results_json,
            block_timestamp = EXCLUDED.block_timestamp",
        |row, values| {
            values.push(row.id.clone().into());
            values.push(row.block_height.into());
            values.push(row.shard_id.into());
            values.push(row.receipt_id.clone().into());
            values.push(row.outcome.clone().into());
            values.push(row.raw_data.clone().into());
            values.push(row.transaction_hash.clone().into());
            values.push(row.block_hash.clone().into());
            values.push(row.chunk_hash.clone().into());
            values.push(row.gas_burnt.into());
            values.push(row.tokens_burnt_yocto.clone().into());
            values.push(row.executor_account_id.clone().into());
            values.push(row.status.clone().into());
            values.push(row.outcome_receipt_ids.clone().into());
            values.push(row.executed_in_block_hash.clone().into());
            values.push(row.logs.clone().into());
            values.push(row.results_json.clone().into());
            values.push(row.block_timestamp.into());
        },
        "failed to bulk upsert near_execution_outcomes",
    )
    .await
}

pub async fn flush_near_receipt_actions(
    db: &impl ConnectionTrait,
    rows: &[NearReceiptActionRow],
) -> Result<()> {
    execute_chunked_upsert(
        db,
        rows,
        CANONICAL_RECEIPT_ACTION_BULK_INSERT_PLAN,
        "INSERT INTO near.near_receipt_actions (
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
        ) VALUES ",
        "
        ON CONFLICT (id) DO UPDATE SET
            block_height = EXCLUDED.block_height,
            receipt_id = EXCLUDED.receipt_id,
            execution_outcome_id = EXCLUDED.execution_outcome_id,
            transaction_hash = COALESCE(EXCLUDED.transaction_hash, near.near_receipt_actions.transaction_hash),
            shard_id = EXCLUDED.shard_id,
            signer_account_id = EXCLUDED.signer_account_id,
            signer_public_key = EXCLUDED.signer_public_key,
            gas_price = EXCLUDED.gas_price,
            action_kind = EXCLUDED.action_kind,
            action_json = EXCLUDED.action_json,
            predecessor_id = EXCLUDED.predecessor_id,
            receiver_id = EXCLUDED.receiver_id,
            block_hash = EXCLUDED.block_hash,
            chunk_hash = EXCLUDED.chunk_hash,
            author = EXCLUDED.author,
            method_name = EXCLUDED.method_name,
            gas = EXCLUDED.gas,
            deposit = EXCLUDED.deposit,
            args_base64 = EXCLUDED.args_base64,
            args_json = EXCLUDED.args_json,
            action_index = EXCLUDED.action_index,
            block_timestamp = EXCLUDED.block_timestamp",
        |row, values| {
            values.push(row.id.clone().into());
            values.push(row.block_height.into());
            values.push(row.receipt_id.clone().into());
            values.push(row.execution_outcome_id.clone().into());
            values.push(row.transaction_hash.clone().into());
            values.push(row.shard_id.into());
            values.push(row.signer_account_id.clone().into());
            values.push(row.signer_public_key.clone().into());
            values.push(row.gas_price.clone().into());
            values.push(row.action_kind.clone().into());
            values.push(row.action_json.clone().into());
            values.push(row.predecessor_id.clone().into());
            values.push(row.receiver_id.clone().into());
            values.push(row.block_hash.clone().into());
            values.push(row.chunk_hash.clone().into());
            values.push(row.author.clone().into());
            values.push(row.method_name.clone().into());
            values.push(row.gas.into());
            values.push(row.deposit.clone().into());
            values.push(row.args_base64.clone().into());
            values.push(row.args_json.clone().into());
            values.push(row.action_index.into());
            values.push(row.block_timestamp.into());
        },
        "failed to bulk upsert near_receipt_actions",
    )
    .await
}

pub async fn flush_near_receipt_edges(
    db: &impl ConnectionTrait,
    rows: &[NearReceiptEdgeRow],
) -> Result<()> {
    execute_chunked_upsert(
        db,
        rows,
        CANONICAL_RECEIPT_EDGE_BULK_INSERT_PLAN,
        "INSERT INTO near.near_receipt_edges (
            parent_receipt_id,
            child_receipt_id,
            parent_execution_outcome_id,
            block_height,
            block_hash,
            block_timestamp,
            transaction_hash
        ) VALUES ",
        "
        ON CONFLICT (parent_receipt_id, child_receipt_id) DO UPDATE SET
            parent_execution_outcome_id = EXCLUDED.parent_execution_outcome_id,
            block_height = EXCLUDED.block_height,
            block_hash = EXCLUDED.block_hash,
            block_timestamp = EXCLUDED.block_timestamp,
            transaction_hash = COALESCE(
                near.near_receipt_edges.transaction_hash,
                EXCLUDED.transaction_hash
            )",
        |row, values| {
            values.push(row.parent_receipt_id.clone().into());
            values.push(row.child_receipt_id.clone().into());
            values.push(row.parent_execution_outcome_id.clone().into());
            values.push(row.block_height.into());
            values.push(row.block_hash.clone().into());
            values.push(row.block_timestamp.into());
            values.push(row.transaction_hash.clone().into());
        },
        "failed to bulk upsert near_receipt_edges",
    )
    .await
}

pub async fn flush_near_state_changes(
    db: &impl ConnectionTrait,
    rows: &[NearStateChangesRow],
) -> Result<()> {
    execute_chunked_upsert(
        db,
        rows,
        CANONICAL_STATE_CHANGE_BULK_INSERT_PLAN,
        "INSERT INTO near.near_state_changes (
            block_height,
            shard_id,
            state_changes
        ) VALUES ",
        "
        ON CONFLICT (block_height, shard_id) DO NOTHING",
        |row, values| {
            values.push(row.block_height.into());
            values.push(row.shard_id.into());
            values.push(row.state_changes.clone().into());
        },
        "failed to bulk insert near_state_changes",
    )
    .await
}
