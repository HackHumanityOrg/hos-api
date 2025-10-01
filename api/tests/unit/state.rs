use api::state::AppState;
use sea_orm::{DatabaseBackend, MockDatabase};
use std::sync::Arc;

#[test]
fn app_state_without_db_is_none() {
    let state = AppState::new(None);
    assert!(state.db.is_none());
}

#[test]
fn app_state_with_db_wraps_connection_in_arc() {
    let conn = MockDatabase::new(DatabaseBackend::MySql).into_connection();
    let state = AppState::from_database(Some(conn));
    let shared = state.db.expect("db handle");

    assert_eq!(Arc::strong_count(&shared), 1);

    let cloned_state = AppState::new(Some(shared.clone()));
    assert!(cloned_state.db.is_some());
    assert_eq!(Arc::strong_count(&shared), 2);
}
