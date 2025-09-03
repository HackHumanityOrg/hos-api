use migration::{sea_orm, Migrator, MigratorTrait};
use sea_orm::DatabaseConnection;
use std::sync::Once;
use testcontainers::{runners::AsyncRunner, ContainerAsync, ImageExt};
use testcontainers_modules::mysql::Mysql;

pub struct TestDatabase {
    pub connection: DatabaseConnection,
    pub _container: ContainerAsync<Mysql>,
}

impl TestDatabase {
    pub async fn new() -> Self {
        Self::new_with(
            "data_commons_test",
            "test_user",
            "test_password",
            "root_password",
        )
        .await
    }

    pub async fn new_with(db: &str, user: &str, pass: &str, root: &str) -> Self {
        let mysql_container = Mysql::default()
            .with_env_var("MYSQL_DATABASE", db)
            .with_env_var("MYSQL_USER", user)
            .with_env_var("MYSQL_PASSWORD", pass)
            .with_env_var("MYSQL_ROOT_PASSWORD", root)
            .start()
            .await
            .expect("Failed to start MySQL container");

        let mut port: Option<u16> = None;
        for _ in 0..20u32 {
            match mysql_container.get_host_port_ipv4(3306).await {
                Ok(p) => {
                    port = Some(p);
                    break;
                }
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(250)).await,
            }
        }
        let port = port.expect("Failed to get MySQL port");

        let database_url = format!("mysql://{}:{}@127.0.0.1:{}/{}", user, pass, port, db);
        let connection = Self::connect_with_retry(&database_url, 20).await;

        // Run migrations with a brief retry path for transient startup
        if let Err(e) = Migrator::up(&connection, None).await {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            if let Err(e2) = Migrator::up(&connection, None).await {
                panic!("Failed to run migrations: {}; second attempt: {}", e, e2);
            }
        }

        Self {
            connection,
            _container: mysql_container,
        }
    }

    async fn connect_with_retry(database_url: &str, max_attempts: u32) -> DatabaseConnection {
        for attempt in 1..=max_attempts {
            match sea_orm::Database::connect(database_url).await {
                Ok(conn) => return conn,
                Err(e) => {
                    if attempt == max_attempts {
                        panic!("Failed DB connect after {} attempts: {}", max_attempts, e);
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(700)).await;
                }
            }
        }
        unreachable!()
    }

    pub fn db(&self) -> DatabaseConnection {
        self.connection.clone()
    }
}

pub fn init_test_tracing() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter("debug")
            .with_test_writer()
            .init();
    });
}
