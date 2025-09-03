use migration::{Migrator, MigratorTrait};
use sea_orm::Database;
use testcontainers::runners::AsyncRunner;
use testcontainers::ImageExt;
use testcontainers_modules::mysql::Mysql;

#[tokio::test]
async fn test_migrator_up_on_fresh_mysql() {
    // Start MySQL container
    let container = Mysql::default()
        .with_env_var("MYSQL_DATABASE", "migration_test")
        .with_env_var("MYSQL_USER", "user")
        .with_env_var("MYSQL_PASSWORD", "pass")
        .with_env_var("MYSQL_ROOT_PASSWORD", "root")
        .start()
        .await
        .expect("start mysql");

    // Try to get port with retries
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
    let port = port.expect("published port");

    // Connect with retry
    let url = format!("mysql://user:pass@127.0.0.1:{}/migration_test", port);
    let conn = connect_with_retry(&url, 20).await;

    // Run migrations; should succeed
    Migrator::up(&conn, None).await.expect("migrations up");
}

async fn connect_with_retry(url: &str, max_attempts: u32) -> sea_orm::DatabaseConnection {
    for attempt in 1..=max_attempts {
        match Database::connect(url).await {
            Ok(c) => return c,
            Err(e) if attempt == max_attempts => panic!("connect failed: {}", e),
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(700)).await,
        }
    }
    unreachable!()
}
