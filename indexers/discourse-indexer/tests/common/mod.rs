use migration::{sea_orm, MigratorTrait};
use sea_orm::DatabaseConnection;
use testcontainers::{runners::AsyncRunner, ContainerAsync, ImageExt};
use testcontainers_modules::mysql::Mysql;

pub struct TestDatabase {
    pub connection: DatabaseConnection,
    pub _container: ContainerAsync<Mysql>,
}

impl TestDatabase {
    pub async fn new() -> Self {
        let mysql_container = Mysql::default()
            .with_env_var("MYSQL_DATABASE", "discourse_test_db")
            .with_env_var("MYSQL_USER", "discourse_user")
            .with_env_var("MYSQL_PASSWORD", "discourse_password")
            .with_env_var("MYSQL_ROOT_PASSWORD", "root_password")
            .start()
            .await
            .expect("Failed to start MySQL container");

        let port = mysql_container
            .get_host_port_ipv4(3306)
            .await
            .expect("Failed to get MySQL port");

        let database_url = format!(
            "mysql://discourse_user:discourse_password@127.0.0.1:{}/discourse_test_db",
            port
        );

        let connection = Self::connect_with_retry(&database_url, 20).await;

        migration::Migrator::up(&connection, None)
            .await
            .expect("Failed to run migrations");

        Self {
            connection,
            _container: mysql_container,
        }
    }

    async fn connect_with_retry(database_url: &str, max_attempts: u32) -> DatabaseConnection {
        for attempt in 1..=max_attempts {
            match sea_orm::Database::connect(database_url).await {
                Ok(conn) => return conn,
                Err(_) if attempt < max_attempts => {
                    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
                }
                Err(e) => panic!("Failed DB connect after {} attempts: {}", max_attempts, e),
            }
        }
        unreachable!()
    }

    pub fn db(&self) -> DatabaseConnection {
        self.connection.clone()
    }
}
