use near_indexer::IndexerModule;
use near_lake_framework::near_indexer_primitives as nip;
use nip::views as v;
use sea_orm::{EntityTrait, PaginatorTrait};
use std::str::FromStr;
use test_support::TestDatabase;

mod testutil;
use testutil::make_block;

#[tokio::test]
async fn delete_account_sets_beneficiary_on_deletion_row() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    let receipt = v::ReceiptView {
        predecessor_id: "who.near".parse().unwrap(),
        receiver_id: "dead.near".parse().unwrap(),
        receipt_id: nip::CryptoHash::default(),
        receipt: v::ReceiptEnumView::Action {
            signer_id: "who.near".parse().unwrap(),
            signer_public_key: near_crypto::PublicKey::from_str(
                "ed25519:11111111111111111111111111111111",
            )
            .unwrap(),
            gas_price: 0,
            output_data_receivers: vec![],
            input_data_ids: vec![],
            actions: vec![v::ActionView::DeleteAccount {
                beneficiary_id: "benef.near".parse().unwrap(),
            }],
            is_promise_yield: false,
        },
        priority: 0,
    };
    let reo = nip::IndexerExecutionOutcomeWithReceipt {
        execution_outcome: v::ExecutionOutcomeWithIdView {
            proof: vec![],
            block_hash: nip::CryptoHash::default(),
            id: nip::CryptoHash::default(),
            outcome: v::ExecutionOutcomeView {
                logs: vec![],
                receipt_ids: vec![],
                gas_burnt: 0,
                tokens_burnt: 0,
                executor_id: "dead.near".parse().unwrap(),
                status: v::ExecutionStatusView::Unknown,
                metadata: Default::default(),
            },
        },
        receipt,
    };
    let sc_del = v::StateChangeWithCauseView {
        cause: v::StateChangeCauseView::ReceiptProcessing {
            receipt_hash: nip::CryptoHash::default(),
        },
        value: v::StateChangeValueView::AccountDeletion {
            account_id: "dead.near".parse().unwrap(),
        },
    };
    let shard = nip::IndexerShard {
        shard_id: 0u64.into(),
        chunk: None,
        receipt_execution_outcomes: vec![reo],
        state_changes: vec![sc_del],
    };
    let mut msg = make_block(1, 2);
    msg.shards.push(shard);
    let cfg = common::config::AppConfig::Indexer(common::config::IndexerConfig {
        database: common::config::DatabaseConfig {
            url: String::new(),
            max_connections: None,
            min_connections: None,
            connection_timeout: None,
            acquire_timeout: None,
            idle_timeout: None,
            max_lifetime: None,
        },
        logging: common::config::LoggingConfig {
            level: "debug".into(),
            format: None,
        },
        telemetry: common::config::TelemetryConfig {
            enabled: false,
            service_version: "test".into(),
            environment: "test".into(),
            enable_metrics: false,
            enable_traces: false,
            enable_logs: false,
            batch_size: 0,
            export_timeout_sec: 0,
            export_interval_sec: 0,
            resource_attributes: Default::default(),
        },
        discourse: None,
        near: Some(common::config::NearLakeConfig::default()),
    });
    let module = near_indexer::modules::accounts::AccountsModule::new(&cfg);
    module.process_block(&msg, &db).await.unwrap();
    let acct = entities::near_accounts::Entity::find_by_id("dead.near".to_string())
        .one(&db)
        .await
        .unwrap()
        .expect("row");
    assert_eq!(acct.deleted_beneficiary_id.as_deref(), Some("benef.near"));
}

#[tokio::test]
async fn accounts_process_block_persists_basic_state_changes() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    // Only include AccessKey and Data updates to avoid MySQL INSERT IGNORE paths in state history
    let ak = v::AccessKeyView {
        nonce: 1,
        permission: v::AccessKeyPermissionView::FullAccess,
    };
    let sc2 = v::StateChangeWithCauseView {
        cause: v::StateChangeCauseView::ReceiptProcessing {
            receipt_hash: nip::CryptoHash::default(),
        },
        value: v::StateChangeValueView::AccessKeyUpdate {
            account_id: "alice.near".parse().unwrap(),
            public_key: near_crypto::PublicKey::from_str(
                "ed25519:11111111111111111111111111111111",
            )
            .unwrap(),
            access_key: ak,
        },
    };
    let sc3 = v::StateChangeWithCauseView {
        cause: v::StateChangeCauseView::ReceiptProcessing {
            receipt_hash: nip::CryptoHash::default(),
        },
        value: v::StateChangeValueView::DataUpdate {
            account_id: "alice.near".parse().unwrap(),
            key: vec![1, 2, 3].into(),
            value: vec![9, 9].into(),
        },
    };

    let shard = nip::IndexerShard {
        shard_id: 0u64.into(),
        chunk: None,
        receipt_execution_outcomes: vec![],
        state_changes: vec![sc2, sc3],
    };
    let mut msg = make_block(50, 60);
    msg.shards.push(shard);

    let cfg = common::config::AppConfig::Indexer(common::config::IndexerConfig {
        database: common::config::DatabaseConfig {
            url: String::new(),
            max_connections: None,
            min_connections: None,
            connection_timeout: None,
            acquire_timeout: None,
            idle_timeout: None,
            max_lifetime: None,
        },
        logging: common::config::LoggingConfig {
            level: "debug".into(),
            format: None,
        },
        telemetry: common::config::TelemetryConfig {
            enabled: false,
            service_version: "test".into(),
            environment: "test".into(),
            enable_metrics: false,
            enable_traces: false,
            enable_logs: false,
            batch_size: 0,
            export_timeout_sec: 0,
            export_interval_sec: 0,
            resource_attributes: Default::default(),
        },
        discourse: None,
        near: Some(common::config::NearLakeConfig::default()),
    });
    let module = near_indexer::modules::accounts::AccountsModule::new(&cfg);
    module.process_block(&msg, &db).await.unwrap();

    let ak_count = entities::near_access_keys::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(ak_count, 1);
    let data_count = entities::near_contract_data::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(data_count, 1);
}
