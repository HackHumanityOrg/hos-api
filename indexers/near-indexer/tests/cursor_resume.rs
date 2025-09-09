use near_indexer::core::cursor::{get_module_cursor, initialize_cursor_storage, reset_cursor_storage, upsert_module_cursor};
use sea_orm::{ActiveValue::NotSet, EntityTrait, Set};
pub use test_support::TestDatabase;

#[tokio::test]
async fn get_module_cursor_returns_correct_height() {
    reset_cursor_storage();
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    // Insert a cursor at height 100 directly in DB
    let row = entities::near_sync_cursors::ActiveModel {
        stream_name: Set("near-indexer:accounts".to_string()),
        block_height: Set(100),
        updated_at: NotSet,
    };
    let _ = entities::near_sync_cursors::Entity::insert(row)
        .exec(&db)
        .await
        .expect("insert cursor");

    // Initialize cursor storage to load from DB
    initialize_cursor_storage(&db).await.expect("initialize");

    // Should return the cursor height from memory
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
    reset_cursor_storage();
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    // Initialize cursor storage
    initialize_cursor_storage(&db).await.expect("initialize");

    // First upsert should insert and set updated_at by DB default
    upsert_module_cursor(&db, "test_accounts", 10)
        .await
        .expect("upsert insert");
    let first =
        entities::near_sync_cursors::Entity::find_by_id("near-indexer:test_accounts".to_string())
            .one(&db)
            .await
            .expect("select1")
            .expect("exists1");
    assert_eq!(first.block_height, 10);
    assert!(first.updated_at.is_some());

    // Verify in-memory value is updated
    let cursor = get_module_cursor(&db, "test_accounts").await.expect("get cursor");
    assert_eq!(cursor, Some(10));

    // Wait more than 1 second to bypass rate limiting
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    // Second upsert should update height and bump updated_at
    upsert_module_cursor(&db, "test_accounts", 11)
        .await
        .expect("upsert update");
    let second =
        entities::near_sync_cursors::Entity::find_by_id("near-indexer:test_accounts".to_string())
            .one(&db)
            .await
            .expect("select2")
            .expect("exists2");
    assert_eq!(second.block_height, 11);
    assert!(second.updated_at.unwrap() > first.updated_at.unwrap());

    // Verify in-memory value is updated
    let cursor = get_module_cursor(&db, "test_accounts").await.expect("get cursor");
    assert_eq!(cursor, Some(11));
}

#[tokio::test]
async fn negative_cursor_can_be_retrieved() {
    reset_cursor_storage();
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

    // Initialize cursor storage to load from DB
    initialize_cursor_storage(&db).await.expect("initialize");

    // Should return the negative cursor value from memory
    let cursor = get_module_cursor(&db, "test")
        .await
        .expect("get negative cursor");
    assert_eq!(cursor, Some(-5));
}

#[tokio::test]
async fn upsert_rate_limiting_works() {
    reset_cursor_storage();
    let test_db = TestDatabase::new().await;
    let db = test_db.db();

    // Initialize cursor storage
    initialize_cursor_storage(&db).await.expect("initialize");

    // First upsert should write to DB
    upsert_module_cursor(&db, "test_module", 100)
        .await
        .expect("upsert 1");

    let first = entities::near_sync_cursors::Entity::find_by_id("near-indexer:test_module".to_string())
        .one(&db)
        .await
        .expect("select1")
        .expect("exists1");
    assert_eq!(first.block_height, 100);

    // Immediate second upsert should update memory but not DB (rate limited)
    upsert_module_cursor(&db, "test_module", 101)
        .await
        .expect("upsert 2");

    // Memory should be updated
    let cursor = get_module_cursor(&db, "test_module").await.expect("get cursor");
    assert_eq!(cursor, Some(101));

    // But DB should still be at old value (rate limited)
    let db_cursor = entities::near_sync_cursors::Entity::find_by_id("near-indexer:test_module".to_string())
        .one(&db)
        .await
        .expect("select2")
        .expect("exists2");
    assert_eq!(db_cursor.block_height, 100); // Still old value

    // Wait for rate limit to pass
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;

    // Third upsert should now write to DB
    upsert_module_cursor(&db, "test_module", 102)
        .await
        .expect("upsert 3");

    let final_cursor = entities::near_sync_cursors::Entity::find_by_id("near-indexer:test_module".to_string())
        .one(&db)
        .await
        .expect("select3")
        .expect("exists3");
    assert_eq!(final_cursor.block_height, 102); // Now updated
}
