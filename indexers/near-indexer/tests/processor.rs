use anyhow::{anyhow, Result};
use near_indexer::core::processor::{process_streamer_message, run_processing_loop};
use near_indexer::{IndexerModule, ModuleRegistry};
use near_lake_framework::near_indexer_primitives as nip;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use test_support::TestDatabase;

mod testutil;
use testutil::make_block;

#[derive(Clone)]
struct TestModule {
    name: &'static str,
    enabled: bool,
    fail_for: Arc<AtomicUsize>,
    processed: Arc<AtomicUsize>,
    updated: Arc<AtomicUsize>,
}

#[async_trait::async_trait]
impl IndexerModule for TestModule {
    fn name(&self) -> &str {
        self.name
    }
    fn enabled(&self) -> bool {
        self.enabled
    }
    async fn process_block(
        &self,
        _msg: &nip::StreamerMessage,
        _db: &sea_orm::DatabaseConnection,
    ) -> Result<()> {
        let n = self.fail_for.load(Ordering::SeqCst);
        if n > 0 {
            self.fail_for.fetch_sub(1, Ordering::SeqCst);
            return Err(anyhow!("fail as requested"));
        }
        self.processed.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn get_cursor(&self, _db: &sea_orm::DatabaseConnection) -> Result<Option<i64>> {
        Ok(None)
    }
    async fn update_cursor(&self, _db: &sea_orm::DatabaseConnection, _height: i64) -> Result<()> {
        self.updated.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn process_streamer_message_runs_enabled_modules_and_updates_cursors() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    let m1 = TestModule {
        name: "m1",
        enabled: true,
        fail_for: Arc::new(AtomicUsize::new(0)),
        processed: Arc::new(AtomicUsize::new(0)),
        updated: Arc::new(AtomicUsize::new(0)),
    };
    let m2 = TestModule {
        name: "m2",
        enabled: true,
        fail_for: Arc::new(AtomicUsize::new(0)),
        processed: Arc::new(AtomicUsize::new(0)),
        updated: Arc::new(AtomicUsize::new(0)),
    };

    let mut registry = ModuleRegistry::new();
    registry.register_module(Box::new(m1.clone()));
    registry.register_module(Box::new(m2.clone()));

    let msg = make_block(123, 456);
    process_streamer_message(&registry, &db, msg).await.unwrap();

    assert_eq!(m1.processed.load(Ordering::SeqCst), 1);
    assert_eq!(m2.processed.load(Ordering::SeqCst), 1);
    assert_eq!(m1.updated.load(Ordering::SeqCst), 1);
    assert_eq!(m2.updated.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn run_processing_loop_retries_until_success_with_backoff() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    let failing = TestModule {
        name: "flaky",
        enabled: true,
        fail_for: Arc::new(AtomicUsize::new(2)),
        processed: Arc::new(AtomicUsize::new(0)),
        updated: Arc::new(AtomicUsize::new(0)),
    };

    let mut registry = ModuleRegistry::new();
    registry.register_module(Box::new(failing.clone()));
    let registry = Arc::new(registry);

    let (tx, rx) = tokio::sync::mpsc::channel(1);
    let msg = make_block(999, 111);
    tx.send(msg.clone()).await.unwrap();
    drop(tx);

    let loop_res = run_processing_loop(registry, db, rx).await;
    assert!(loop_res.is_ok());
    assert_eq!(failing.processed.load(Ordering::SeqCst), 1);
    assert_eq!(failing.updated.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn process_streamer_message_stops_on_error() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    let failing = TestModule {
        name: "fail_first",
        enabled: true,
        fail_for: Arc::new(AtomicUsize::new(1)),
        processed: Arc::new(AtomicUsize::new(0)),
        updated: Arc::new(AtomicUsize::new(0)),
    };
    let later = TestModule {
        name: "later",
        enabled: true,
        fail_for: Arc::new(AtomicUsize::new(0)),
        processed: Arc::new(AtomicUsize::new(0)),
        updated: Arc::new(AtomicUsize::new(0)),
    };

    let mut registry = ModuleRegistry::new();
    registry.register_module(Box::new(failing.clone()));
    registry.register_module(Box::new(later.clone()));
    let msg = make_block(1, 2);
    let _ = process_streamer_message(&registry, &db, msg)
        .await
        .unwrap_err();
    assert_eq!(failing.processed.load(Ordering::SeqCst), 0);
    assert_eq!(later.processed.load(Ordering::SeqCst), 0);
    assert_eq!(failing.updated.load(Ordering::SeqCst), 0);
    assert_eq!(later.updated.load(Ordering::SeqCst), 0);
}
