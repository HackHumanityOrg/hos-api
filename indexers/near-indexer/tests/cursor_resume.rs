use near_indexer::core::cursor::{choose_module_start_height, upsert_module_cursor};
use sea_orm::{ActiveValue::NotSet, EntityTrait, Set};
pub use test_support::TestDatabase;

#[tokio::test]
async fn choose_module_start_height_prefers_cursor_plus_one() {
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

    // Configured start lower than cursor+1; expect 101
    let (start, used_cursor) = choose_module_start_height(&db, "accounts", 90)
        .await
        .expect("choose");
    assert!(used_cursor);
    assert_eq!(start, 101);

    // Configured start higher than cursor+1; expect configured start
    let (start2, used_cursor2) = choose_module_start_height(&db, "accounts", 200)
        .await
        .expect("choose2");
    assert!(used_cursor2);
    assert_eq!(start2, 200);
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
