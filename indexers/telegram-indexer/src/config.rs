use anyhow::{Context, Result};
use common::config::AppConfig;

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub api_id: i32,
    pub api_hash: String,
    pub session_file: String,
    pub session_data: Option<String>,
    pub channels: Vec<String>,
    pub sync_batch_size: usize,
    pub max_concurrent_channels: usize,
    pub max_retry_attempts: usize,
    pub retry_base_delay_seconds: u64,
    pub max_messages_to_sync: Option<usize>,
    pub force_full_backfill: bool,
    pub allow_interactive_login: bool,
}

impl RuntimeConfig {
    pub fn from_app(config: &AppConfig) -> Result<Self> {
        let telegram = config.telegram().cloned();

        let api_id = env_parse::<i32>("TELEGRAM_API_ID")
            .or_else(|| telegram.as_ref().and_then(|t| t.api_id))
            .context(
                "Missing Telegram API id: set APP_TELEGRAM__API_ID or TELEGRAM_API_ID environment variable",
            )?;

        let api_hash = env_opt("TELEGRAM_API_HASH")
            .or_else(|| telegram.as_ref().and_then(|t| t.api_hash.clone()))
            .filter(|value| !value.trim().is_empty())
            .context(
                "Missing Telegram API hash: set APP_TELEGRAM__API_HASH or TELEGRAM_API_HASH environment variable",
            )?;

        let session_file = env_opt("TELEGRAM_SESSION_FILE")
            .or_else(|| telegram.as_ref().map(|t| t.session_file.clone()))
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "telegram_session.bin".to_string());

        let session_data = env_opt("TELEGRAM_SESSION_DATA")
            .or_else(|| telegram.as_ref().and_then(|t| t.session_data.clone()));

        let channels = {
            let configured = telegram
                .as_ref()
                .map(|t| t.channels.clone())
                .unwrap_or_default();

            if configured.is_empty() {
                env_opt("TELEGRAM_CHANNELS")
                    .map(|raw| parse_channels(&raw))
                    .unwrap_or_default()
            } else {
                configured
            }
        };

        let sync_batch_size = env_parse::<usize>("SYNC_BATCH_SIZE")
            .or_else(|| telegram.as_ref().map(|t| t.sync_batch_size))
            .unwrap_or(100);

        let max_concurrent_channels = env_parse::<usize>("MAX_CONCURRENT_CHANNELS")
            .or_else(|| telegram.as_ref().map(|t| t.max_concurrent_channels))
            .unwrap_or(5);

        let max_retry_attempts = env_parse::<usize>("MAX_RETRY_ATTEMPTS")
            .or_else(|| telegram.as_ref().map(|t| t.max_retry_attempts))
            .unwrap_or(3);

        let retry_base_delay_seconds = env_parse::<u64>("RETRY_BASE_DELAY_SECONDS")
            .or_else(|| telegram.as_ref().map(|t| t.retry_base_delay_seconds))
            .unwrap_or(60);

        let max_messages_to_sync = env_parse::<usize>("MAX_MESSAGES_TO_SYNC")
            .or_else(|| telegram.as_ref().and_then(|t| t.max_messages_to_sync));

        let force_full_backfill = env_opt("TELEGRAM_FORCE_FULL_BACKFILL")
            .map(|value| parse_bool(&value))
            .unwrap_or(false);

        let allow_interactive_login = env_opt("TELEGRAM_ALLOW_INTERACTIVE_LOGIN")
            .map(|value| parse_bool(&value))
            .or_else(|| telegram.as_ref().map(|t| t.allow_interactive_login))
            .unwrap_or(false);

        let runtime = Self {
            api_id,
            api_hash,
            session_file,
            session_data,
            channels,
            sync_batch_size,
            max_concurrent_channels,
            max_retry_attempts,
            retry_base_delay_seconds,
            max_messages_to_sync,
            force_full_backfill,
            allow_interactive_login,
        };

        runtime.validate()?;
        Ok(runtime)
    }

    fn validate(&self) -> Result<()> {
        if self.api_id <= 0 {
            anyhow::bail!("Telegram api_id must be a positive integer");
        }

        if self.api_hash.trim().is_empty() {
            anyhow::bail!("Telegram api_hash cannot be empty");
        }

        if self.channels.is_empty() {
            anyhow::bail!(
                "No Telegram channels configured. Set telegram.channels in config.toml or TELEGRAM_CHANNELS"
            );
        }

        if self.sync_batch_size == 0 || self.sync_batch_size > 1_000 {
            anyhow::bail!("sync_batch_size must be between 1 and 1000");
        }

        if self.max_concurrent_channels == 0 {
            anyhow::bail!("max_concurrent_channels must be greater than 0");
        }

        if self.max_retry_attempts == 0 {
            anyhow::bail!("max_retry_attempts must be greater than 0");
        }

        if self.retry_base_delay_seconds == 0 {
            anyhow::bail!("retry_base_delay_seconds must be greater than 0");
        }

        Ok(())
    }
}

fn env_opt(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_parse<T: std::str::FromStr>(name: &str) -> Option<T> {
    env_opt(name).and_then(|value| value.parse::<T>().ok())
}

fn parse_channels(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

fn parse_bool(raw: &str) -> bool {
    matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_bool, parse_channels};

    #[test]
    fn parse_channels_splits_and_trims() {
        let channels = parse_channels("@near,  -100123 ,gov.near");
        assert_eq!(channels, vec!["@near", "-100123", "gov.near"]);
    }

    #[test]
    fn parse_bool_accepts_true_values() {
        assert!(parse_bool("true"));
        assert!(parse_bool("Yes"));
        assert!(parse_bool("1"));
        assert!(!parse_bool("false"));
        assert!(!parse_bool("0"));
    }
}
