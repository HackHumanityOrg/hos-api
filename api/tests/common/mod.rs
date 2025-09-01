// Common testing utilities and fixtures
use axum_test::TestServer;
use migration::{sea_orm, MigratorTrait};
use sea_orm::DatabaseConnection;
use testcontainers::{runners::AsyncRunner, ImageExt};
use testcontainers_modules::mysql::Mysql;

/// Test database container setup
pub struct TestDatabase {
    pub connection: DatabaseConnection,
    // Keep container alive for the duration of the test
    pub _container: testcontainers::ContainerAsync<testcontainers_modules::mysql::Mysql>,
}

impl TestDatabase {
    /// Create a new test database with MySQL container
    pub async fn new() -> Self {
        // Start MySQL container
        let mysql_container = Mysql::default()
            .with_env_var("MYSQL_DATABASE", "test_db")
            .with_env_var("MYSQL_USER", "test_user")
            .with_env_var("MYSQL_PASSWORD", "test_password")
            .with_env_var("MYSQL_ROOT_PASSWORD", "root_password")
            .start()
            .await
            .expect("Failed to start MySQL container");

        let port = mysql_container
            .get_host_port_ipv4(3306)
            .await
            .expect("Failed to get MySQL port");

        // Create database URL
        let database_url = format!("mysql://test_user:test_password@127.0.0.1:{}/test_db", port);

        // Create database connection
        let connection = sea_orm::Database::connect(&database_url)
            .await
            .expect("Failed to connect to test database");

        // Run migrations
        if let Err(e) = migration::Migrator::up(&connection, None).await {
            panic!("Failed to run migrations: {}", e);
        }

        TestDatabase {
            connection,
            _container: mysql_container,
        }
    }

    /// Get a clone of the database connection for testing
    pub fn db(&self) -> DatabaseConnection {
        self.connection.clone()
    }
}

/// Initialize test tracing (run once per test suite)
pub fn init_test_tracing() {
    use std::sync::Once;
    static INIT: Once = Once::new();

    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_env_filter("debug")
            .with_test_writer()
            .init();
    });
}

/// Create a test server with optional database connection using the health route
pub async fn create_test_server_with_db(db: Option<DatabaseConnection>) -> TestServer {
    use api::handlers::health;
    use api::state::AppState;
    use axum::{routing::get, Router};

    let app_state = AppState { db };

    let app = Router::new()
        .route("/health", get(health::health_check))
        .with_state(app_state);

    TestServer::new(app).unwrap()
}
