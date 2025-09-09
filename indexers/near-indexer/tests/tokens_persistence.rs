use near_indexer::modules::tokens::TokensModule;
use near_indexer::IndexerModule;
use near_lake_framework::near_indexer_primitives as nip;
use nip::views as v;
use sea_orm::{ActiveValue::NotSet, EntityTrait, PaginatorTrait, Set};
use std::str::FromStr;
use test_support::TestDatabase;

mod testutil;
use testutil::{make_block, outcome_with_logs};

fn cfg_enable_tokens() -> common::config::AppConfig {
    common::config::AppConfig::Indexer(common::config::IndexerConfig {
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
        near: Some(common::config::NearLakeConfig {
            chain: "mainnet".into(),
            start_height: 0,
            concurrency: Some(1),
            region: None,
            modules: Some(common::config::NearModulesConfig {
                accounts: None,
                tokens: Some(common::config::TokensModuleConfig {
                    enabled: true,
                    track_nep141: true,
                    track_nep171: false,
                    track_nep245: true,
                    contracts: vec![],
                }),
                validators: None,
                network: None,
            }),
        }),
    })
}

#[tokio::test]
async fn tokens_process_block_ignores_non_event_logs_and_persists_events() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    let bad = "not an event";
    let good = "EVENT_JSON:{\"standard\":\"nep141\",\"version\":\"1.0.0\",\"event\":\"ft_transfer\",\"data\":[{\"old_owner_id\":\"alice.near\",\"new_owner_id\":\"bob.near\",\"amount\":\"100\"}]}";
    let exec_outcome = outcome_with_logs("token.near", vec![bad, good]);
    let receipt = v::ReceiptView {
        predecessor_id: "alice.near".parse().unwrap(),
        receiver_id: "token.near".parse().unwrap(),
        receipt_id: nip::CryptoHash::default(),
        receipt: v::ReceiptEnumView::Action {
            signer_id: "alice.near".parse().unwrap(),
            signer_public_key: near_crypto::PublicKey::from_str(
                "ed25519:11111111111111111111111111111111",
            )
            .unwrap(),
            gas_price: 0,
            output_data_receivers: vec![],
            input_data_ids: vec![],
            actions: vec![],
            is_promise_yield: false,
        },
        priority: 0,
    };
    let reo = nip::IndexerExecutionOutcomeWithReceipt {
        execution_outcome: exec_outcome,
        receipt,
    };

    let shard = nip::IndexerShard {
        shard_id: 0u64.into(),
        chunk: None,
        receipt_execution_outcomes: vec![reo],
        state_changes: vec![],
    };
    let mut msg = make_block(10, 20);
    msg.shards.push(shard);

    // Pre-insert account and token for FK integrity
    let acct = entities::near_accounts::ActiveModel {
        account_id: Set("token.near".to_string()),
        parent_account_id: Set(Some("near".to_string())),
        depth: Set(2),
        is_top_level: Set(0),
        is_implicit: Set(0),
        is_system: Set(0),
        is_staking_pool: NotSet,
        is_lockup_contract: NotSet,
        storage_paid_at: NotSet,
        global_contract_account_id: Set(None),
        global_contract_hash: Set(None),
        created_at_height: Set(None),
        created_at_time: Set(None),
        created_cause: Set(None),
        created_by_tx_hash: Set(None),
        created_by_receipt_hash: Set(None),
        deleted_at_height: Set(None),
        deleted_at_time: Set(None),
        deleted_beneficiary_id: Set(None),
        deleted_by_tx_hash: Set(None),
        deleted_by_receipt_hash: Set(None),
        first_seen_height: Set(None),
        first_seen_time: Set(None),
        last_seen_height: Set(None),
        last_seen_time: Set(None),
        // New enhanced fields
        is_eth_implicit: Set(0),
        permanent_storage_bytes: Set(None),
        has_eth_wallet_contract: Set(0),
        account_id_length: Set(10), // "token.near".len()
    };
    let _ = entities::near_accounts::Entity::insert(acct)
        .exec(&db)
        .await
        .unwrap();

    // Seed participant accounts
    for acc in ["alice.near", "bob.near"] {
        let row = entities::near_accounts::ActiveModel {
            account_id: Set(acc.to_string()),
            parent_account_id: Set(Some("near".to_string())),
            depth: Set(2),
            is_top_level: Set(0),
            is_implicit: Set(0),
            is_system: Set(0),
            is_staking_pool: Set(0),
            is_lockup_contract: Set(0),
            storage_paid_at: Set(None),
            global_contract_account_id: Set(None),
            global_contract_hash: Set(None),
            created_at_height: Set(None),
            created_at_time: Set(None),
            created_cause: Set(None),
            created_by_tx_hash: Set(None),
            created_by_receipt_hash: Set(None),
            deleted_at_height: Set(None),
            deleted_at_time: Set(None),
            deleted_beneficiary_id: Set(None),
            deleted_by_tx_hash: Set(None),
            deleted_by_receipt_hash: Set(None),
            first_seen_height: Set(None),
            first_seen_time: Set(None),
            last_seen_height: Set(None),
            last_seen_time: Set(None),
            // New enhanced fields
            is_eth_implicit: Set(0),
            permanent_storage_bytes: Set(None),
            has_eth_wallet_contract: Set(0),
            account_id_length: Set(acc.len() as i32),
        };
        let _ = entities::near_accounts::Entity::insert(row)
            .exec(&db)
            .await
            .unwrap();
    }

    let token = entities::near_tokens::ActiveModel {
        contract_id: Set("token.near".to_string()),
        token_standard: Set("nep141".to_string()),
        first_seen_height: Set(None),
        first_seen_time: Set(None),
        last_seen_height: Set(None),
        last_seen_time: Set(None),
    };
    let _ = entities::near_tokens::Entity::insert(token)
        .exec(&db)
        .await
        .unwrap();

    // Seed a dummy transaction matching the fallback tx_hash (receipt_id)
    let dummy_tx_hash = nip::CryptoHash::default().to_string();
    let tx_row = entities::near_transactions::ActiveModel {
        tx_hash: Set(dummy_tx_hash.clone()),
        signer_id: Set("alice.near".to_string()),
        receiver_id: Set("token.near".to_string()),
        block_height: Set(10),
        block_time: Set(20),
        block_hash: Set("0x00".to_string()),
        actions: Set(None),
        status: Set("success".to_string()),
        receipt_ids: Set(None),
        gas_used: Set(Some(0)),
        tokens_burnt: Set(None),
        public_key: NotSet,
        nonce: NotSet,
        executor_id: NotSet,
        priority_fee: NotSet,
        signature: NotSet,
        execution_outcome_id: NotSet,
        logs: NotSet,
        proof: NotSet,
        metadata: NotSet,
    };
    let _ = entities::near_transactions::Entity::insert(tx_row)
        .exec(&db)
        .await
        .unwrap();

    let module = TokensModule::new(&cfg_enable_tokens());
    module.process_block(&msg, &db).await.unwrap();

    let count = entities::near_token_events::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(count, 1);
    let tokens_count = entities::near_tokens::Entity::find()
        .count(&db)
        .await
        .unwrap();
    assert_eq!(tokens_count, 1);
}
