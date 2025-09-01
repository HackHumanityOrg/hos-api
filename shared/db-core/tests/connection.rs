#[tokio::test]
async fn test_from_env_missing_database_url() {
    let key = "DATABASE_URL";
    // Preserve existing value
    let prev = std::env::var(key).ok();
    std::env::remove_var(key);

    let res = db_core::connection::from_env().await;

    match res {
        Err(common::errors::ConfigError::MissingEnvVar { var_name }) => {
            assert_eq!(var_name, "DATABASE_URL");
        }
        other => panic!("Expected MissingEnvVar, got: {:?}", other),
    }

    // Restore previous value
    if let Some(v) = prev {
        std::env::set_var(key, v);
    }
}
