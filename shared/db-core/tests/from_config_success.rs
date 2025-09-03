use db_core::connection::from_config;
use migration::{Migrator, MigratorTrait};
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::mysql::Mysql;

#[tokio::test]
async fn test_from_config_success_with_container() {
    // Start a MySQL container
    let container = Mysql::default()
        .with_env_var("MYSQL_DATABASE", "db_core_test")
        .with_env_var("MYSQL_USER", "user")
        .with_env_var("MYSQL_PASSWORD", "pass")
        .with_env_var("MYSQL_ROOT_PASSWORD", "root")
        .start()
        .await
        .expect("start mysql");

    // Published port may not be ready immediately
    let mut port: Option<u16> = None;
    for _ in 0..20u32 {
        match container.get_host_port_ipv4(3306).await {
            Ok(p) => {
                port = Some(p);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(250)).await,
        }
    }
    let port = port.expect("port");

    // Build config for db-core
    let cfg = common::config::DatabaseConfig {
        url: format!("mysql://user:pass@127.0.0.1:{}/db_core_test", port),
        max_connections: Some(5),
        min_connections: Some(1),
        connection_timeout: Some(5),
        acquire_timeout: Some(5),
        idle_timeout: Some(60),
        max_lifetime: Some(600),
    };

    // Connect using db-core
    let conn = from_config(&cfg).await.expect("connect");

    // Apply migrations to validate the connection can execute statements
    Migrator::up(&conn, None).await.expect("migrate up");
}
