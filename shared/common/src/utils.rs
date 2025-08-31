//! Utility functions shared across the workspace

use tracing::info;

/// Initialize tracing subscriber with sensible defaults
pub fn init_tracing(level: &str) -> anyhow::Result<()> {
    use tracing_subscriber::{EnvFilter, FmtSubscriber};

    let subscriber = FmtSubscriber::builder()
        .with_env_filter(EnvFilter::try_new(level)?)
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Tracing initialized with level: {}", level);
    Ok(())
}
