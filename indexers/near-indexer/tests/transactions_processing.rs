use near_indexer::modules::transactions::TransactionsProcessor;
use near_indexer::IndexerModule;
use near_lake_framework::near_indexer_primitives as nip;
use nip::views as v;
use sea_orm::{EntityTrait, PaginatorTrait};
use std::str::FromStr;
use test_support::TestDatabase;

mod testutil;
use testutil::{chunk_header, make_block};

fn tx_with_outcome(
    hash: nip::CryptoHash,
    signer: &str,
    receiver: &str,
    status: v::ExecutionStatusView,
) -> nip::IndexerTransactionWithOutcome {
    let stx = v::SignedTransactionView {
        signer_id: signer.parse().unwrap(),
        public_key: near_crypto::PublicKey::from_str("ed25519:11111111111111111111111111111111")
            .unwrap(),
        nonce: 1,
        receiver_id: receiver.parse().unwrap(),
        actions: vec![v::ActionView::Transfer { deposit: 0 }],
        priority_fee: 0,
        signature: near_crypto::Signature::empty(near_crypto::KeyType::ED25519),
        hash,
    };
    let eo = v::ExecutionOutcomeWithIdView {
        proof: vec![],
        block_hash: nip::CryptoHash::default(),
        id: nip::CryptoHash::default(),
        outcome: v::ExecutionOutcomeView {
            logs: vec![],
            receipt_ids: vec![nip::CryptoHash::default()],
            gas_burnt: 777,
            tokens_burnt: 1234,
            executor_id: receiver.parse().unwrap(),
            status,
            metadata: Default::default(),
        },
    };
    nip::IndexerTransactionWithOutcome {
        transaction: stx,
        outcome: nip::IndexerExecutionOutcomeWithOptionalReceipt {
            execution_outcome: eo,
            receipt: None,
        },
    }
}

#[tokio::test]
async fn transactions_processor_inserts_row() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    let tx_hash = nip::CryptoHash::default();
    let tx = tx_with_outcome(
        tx_hash,
        "alice.near",
        "contract.near",
        v::ExecutionStatusView::SuccessValue(vec![]),
    );

    let ch = chunk_header(100, 0);
    let shard = nip::IndexerShard {
        shard_id: 0u64.into(),
        chunk: Some(nip::IndexerChunkView {
            author: "prod.near".parse().unwrap(),
            header: ch,
            transactions: vec![tx],
            receipts: vec![],
        }),
        receipt_execution_outcomes: vec![],
        state_changes: vec![],
    };

    let mut msg = make_block(100, 200);
    msg.shards.push(shard);

    let module = TransactionsProcessor::new(true);
    module.process_block(&msg, &db).await.unwrap();

    let count = entities::near_transactions::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(count, 1);
}
