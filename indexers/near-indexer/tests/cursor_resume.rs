use near_indexer::core::cursor::{get_module_cursor, upsert_module_cursor};
use sea_orm::{ActiveValue::NotSet, EntityTrait, Set};
pub use test_support::TestDatabase;

#[tokio::test]
async fn get_module_cursor_returns_correct_height() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    // Insert a cursor at height 100
    let row = entities::near_sync_cursors::ActiveModel {
        stream_name: Set("near-indexer:accounts".to_string()),
        block_height: Set(100),
        updated_at: NotSet,
    };
    let _ = entities::near_sync_cursors::Entity::insert(row)
        .exec(&db)
        .await
        .expect("insert cursor");

    // Should return the cursor height
    let cursor = get_module_cursor(&db, "accounts")
        .await
        .expect("get cursor");
    assert_eq!(cursor, Some(100));

    // Non-existent module should return None
    let no_cursor = get_module_cursor(&db, "nonexistent")
        .await
        .expect("get nonexistent cursor");
    assert_eq!(no_cursor, None);
}

#[tokio::test]
async fn upsert_module_cursor_inserts_then_updates_timestamp() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    // First upsert should insert and set updated_at by DB default
    upsert_module_cursor(&db, "accounts", 10)
        .await
        .expect("upsert insert");
    let first =
        entities::near_sync_cursors::Entity::find_by_id("near-indexer:accounts".to_string())
            .one(&db)
            .await
            .expect("select1")
            .expect("exists1");
    assert_eq!(first.block_height, 10);
    assert!(first.updated_at.is_some());

    // Wait a bit to see timestamp change
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    // Second upsert should update height and bump updated_at
    upsert_module_cursor(&db, "accounts", 11)
        .await
        .expect("upsert update");
    let second =
        entities::near_sync_cursors::Entity::find_by_id("near-indexer:accounts".to_string())
            .one(&db)
            .await
            .expect("select2")
            .expect("exists2");
    assert_eq!(second.block_height, 11);
    assert!(second.updated_at.unwrap() > first.updated_at.unwrap());
}

#[tokio::test]
async fn negative_cursor_can_be_retrieved() {
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    // Insert a negative height cursor
    let row = entities::near_sync_cursors::ActiveModel {
        stream_name: Set("near-indexer:test".to_string()),
        block_height: Set(-5),
        updated_at: NotSet,
    };
    let _ = entities::near_sync_cursors::Entity::insert(row)
        .exec(&db)
        .await
        .expect("insert neg cursor");

    // Should return the negative cursor value
    let cursor = get_module_cursor(&db, "test")
        .await
        .expect("get negative cursor");
    assert_eq!(cursor, Some(-5));
}
