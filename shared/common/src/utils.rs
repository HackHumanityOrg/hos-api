//! Utility functions shared across the workspace

use tracing::info;

/// Initialize tracing subscriber with sensible defaults
pub fn init_tracing(level: &str) -> anyhow::Result<()> {
    use tracing_subscriber::EnvFilter;

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(level)?)
        .json()
        .finish();

    tracing::subscriber::set_global_default(subscriber)?;

    info!("Tracing initialized with level: {}", level);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_tracing_rejects_invalid_level() {
        // Invalid level directive should fail to parse
        let res = init_tracing("*=nope");
        assert!(res.is_err());
    }

    #[test]
    fn init_tracing_accepts_valid_level_once() {
        // Should succeed when no global subscriber is set yet
        let res = init_tracing("info");
        assert!(res.is_ok());
    }
}
