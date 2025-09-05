use crate::core::lake_types::*;
use near_lake_framework::near_indexer_primitives as nip;
use nip::views as v;
use rust_decimal::Decimal;

pub fn build_block(msg: &nip::StreamerMessage) -> AdaptedBlock {
    let header = BlockHeaderView::new_from(msg);

    let mut txs: Vec<TxView> = Vec::new();
    let mut receipt_outcomes: Vec<ReceiptOutcomeView> = Vec::new();
    let mut state_changes: Vec<StateChangeRecord> = Vec::new();
    let mut chunk_producers: Vec<ChunkProducer> = Vec::new();

    // Transactions + tx outcome logs + map receipts to txs implicitly for consumers via receipt_outcomes
    for shard in &msg.shards {
        if let Some(chunk) = &shard.chunk {
            // Producer info if available
            let producer = Some(chunk.author.to_string());
            chunk_producers.push(ChunkProducer {
                shard_id: shard.shard_id.into(),
                chunk_hash: Some(chunk.header.chunk_hash.to_string()),
                producer,
            });

            for txo in &chunk.transactions {
                let tx = &txo.transaction;
                let out = &txo.outcome.execution_outcome.outcome;

                let status = match &out.status {
                    v::ExecutionStatusView::SuccessValue(_) => ExecStatus::Success,
                    v::ExecutionStatusView::SuccessReceiptId(_) => ExecStatus::Success,
                    v::ExecutionStatusView::Failure(_) => ExecStatus::Failure,
                    v::ExecutionStatusView::Unknown => ExecStatus::Unknown,
                };

                let tokens_burnt =
                    Decimal::from_str_exact(&out.tokens_burnt.to_string()).unwrap_or(Decimal::ZERO);

                let tx_view = TxView {
                    hash: tx.hash.to_string(),
                    signer_id: tx.signer_id.to_string(),
                    receiver_id: tx.receiver_id.to_string(),
                    status,
                    gas_burnt: out.gas_burnt as i64,
                    tokens_burnt,
                    actions_json: serde_json::to_string(&tx.actions).unwrap_or("[]".into()),
                    receipt_ids: out.receipt_ids.iter().map(|h| h.to_string()).collect(),
                    executor_id: out.executor_id.to_string(),
                    public_key: Some(tx.public_key.to_string()),
                    nonce: Some(tx.nonce as i64),
                };
                txs.push(tx_view);

                // Also capture tx-outcome logs as a pseudo-receipt outcome
                receipt_outcomes.push(ReceiptOutcomeView {
                    receipt_id: tx.hash.to_string(),
                    executor_id: out.executor_id.to_string(),
                    logs: out.logs.clone(),
                });
            }
        } else {
            chunk_producers.push(ChunkProducer {
                shard_id: shard.shard_id.into(),
                chunk_hash: None,
                producer: None,
            });
        }
    }

    // Receipt execution outcomes
    for shard in &msg.shards {
        for reo in &shard.receipt_execution_outcomes {
            let out = &reo.execution_outcome.outcome;
            receipt_outcomes.push(ReceiptOutcomeView {
                receipt_id: reo.receipt.receipt_id.to_string(),
                executor_id: out.executor_id.to_string(),
                logs: out.logs.clone(),
            });
        }
    }

    // State changes
    for shard in &msg.shards {
        for sc in &shard.state_changes {
            let (cause, cause_tx, cause_receipt) = match &sc.cause {
                v::StateChangeCauseView::NotWritableToDisk => {
                    ("not_writable_to_disk".into(), None, None)
                }
                v::StateChangeCauseView::InitialState => ("initial_state".into(), None, None),
                v::StateChangeCauseView::TransactionProcessing { tx_hash } => (
                    "transaction_processing".into(),
                    Some(tx_hash.to_string()),
                    None,
                ),
                v::StateChangeCauseView::ActionReceiptProcessingStarted { receipt_hash } => (
                    "action_receipt_processing_started".into(),
                    None,
                    Some(receipt_hash.to_string()),
                ),
                v::StateChangeCauseView::ActionReceiptGasReward { receipt_hash } => (
                    "action_receipt_gas_reward".into(),
                    None,
                    Some(receipt_hash.to_string()),
                ),
                v::StateChangeCauseView::ReceiptProcessing { receipt_hash } => (
                    "receipt_processing".into(),
                    None,
                    Some(receipt_hash.to_string()),
                ),
                v::StateChangeCauseView::PostponedReceipt { receipt_hash } => (
                    "postponed_receipt".into(),
                    None,
                    Some(receipt_hash.to_string()),
                ),
                v::StateChangeCauseView::UpdatedDelayedReceipts => {
                    ("updated_delayed_receipts".into(), None, None)
                }
                v::StateChangeCauseView::ValidatorAccountsUpdate => {
                    ("validator_accounts_update".into(), None, None)
                }
                v::StateChangeCauseView::Migration => ("migration".into(), None, None),
                v::StateChangeCauseView::BandwidthSchedulerStateUpdate => {
                    ("bandwidth_scheduler_state_update".into(), None, None)
                }
            };

            state_changes.push(StateChangeRecord {
                cause,
                cause_tx,
                cause_receipt,
                value: sc.value.clone(),
            });
        }
    }

    AdaptedBlock {
        header,
        txs,
        receipt_outcomes,
        state_changes,
        chunk_producers,
    }
}
