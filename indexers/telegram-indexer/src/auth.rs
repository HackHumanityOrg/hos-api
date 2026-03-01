use anyhow::{Context, Result};
use grammers_client::sender::{SenderPool, SenderPoolHandle};
use grammers_client::{Client, SignInError};
use grammers_session::storages::SqliteSession;
use grammers_session::updates::UpdatesLike;
use grammers_session::SessionData;
use grammers_session_legacy::Session as LegacySession;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::config::RuntimeConfig;

pub struct TelegramAuth;

pub struct TelegramConnection {
    pub client: Client,
    updates_rx: Option<mpsc::UnboundedReceiver<UpdatesLike>>,
    pool_handle: SenderPoolHandle,
    runner_task: JoinHandle<()>,
}

impl TelegramConnection {
    pub fn take_updates_rx(&mut self) -> Result<mpsc::UnboundedReceiver<UpdatesLike>> {
        self.updates_rx
            .take()
            .context("Telegram updates receiver was already consumed")
    }

    pub async fn shutdown(self) {
        self.pool_handle.quit();
        if let Err(err) = self.runner_task.await {
            warn!(error = %err, "Telegram sender pool task terminated with error");
        }
    }
}

impl TelegramAuth {
    pub async fn connect(config: &RuntimeConfig) -> Result<TelegramConnection> {
        let max_retries = 60;
        let retry_delay = Duration::from_secs(60);

        for attempt in 1..=max_retries {
            info!(attempt, max_retries, "Telegram authentication attempt");

            let interactive_this_attempt = config.allow_interactive_login && attempt == 1;
            match Self::try_connect(config, interactive_this_attempt).await {
                Ok(connection) => return Ok(connection),
                Err(err) => {
                    error!(attempt, error = %err, "Telegram auth attempt failed");

                    if attempt < max_retries {
                        info!(
                            retry_seconds = retry_delay.as_secs(),
                            session_file = %config.session_file,
                            "Waiting before retry so session file can be uploaded"
                        );
                        sleep(retry_delay).await;
                    } else {
                        return Err(err).context("All Telegram authentication attempts failed");
                    }
                }
            }
        }

        Err(anyhow::anyhow!(
            "Failed to connect to Telegram after all retries"
        ))
    }

    async fn try_connect(
        config: &RuntimeConfig,
        allow_interactive_login: bool,
    ) -> Result<TelegramConnection> {
        Self::prepare_session_file(config).await?;

        let connection = Self::build_connection(config)
            .await
            .context("Failed to build Telegram MTProto client")?;

        if connection.client.is_authorized().await? {
            info!("Telegram session already authorized");
            return Ok(connection);
        }

        if allow_interactive_login {
            if io::stdin().is_terminal() {
                info!("Interactive Telegram login enabled");
                Self::authenticate(&connection.client, &config.api_hash)
                    .await
                    .context("Interactive Telegram login failed")?;

                if connection.client.is_authorized().await? {
                    info!("Interactive Telegram login successful");
                    return Ok(connection);
                }
            } else {
                warn!("Interactive login requested but stdin is not a TTY");
            }
        }

        connection.shutdown().await;

        Err(anyhow::anyhow!(
            "Telegram session is not authorized. Provide TELEGRAM_SESSION_DATA or session file"
        ))
    }

    async fn authenticate(client: &Client, api_hash: &str) -> Result<()> {
        print!("Enter your phone number (with country code, e.g. +1234567890): ");
        io::stdout().flush()?;

        let mut phone = String::new();
        io::stdin().read_line(&mut phone)?;
        let phone = phone.trim().to_string();

        let token = client.request_login_code(&phone, api_hash).await?;

        print!("Enter the Telegram verification code: ");
        io::stdout().flush()?;

        let mut code = String::new();
        io::stdin().read_line(&mut code)?;
        let code = code.trim().to_string();

        match client.sign_in(&token, &code).await {
            Ok(user) => {
                info!(
                    user = %user.first_name().unwrap_or("unknown"),
                    "Signed in to Telegram"
                );
                Ok(())
            }
            Err(SignInError::PasswordRequired(password_token)) => {
                print!("Two-factor password: ");
                io::stdout().flush()?;

                let mut password = String::new();
                io::stdin().read_line(&mut password)?;
                let password = password.trim().to_string();

                client.check_password(password_token, password).await?;
                Ok(())
            }
            Err(err) => Err(err.into()),
        }
    }

    async fn prepare_session_file(config: &RuntimeConfig) -> Result<()> {
        if Path::new(&config.session_file).exists() {
            if Self::is_sqlite_file(&config.session_file).await {
                return Ok(());
            }

            warn!(
                session_file = %config.session_file,
                "Existing Telegram session file is not SQLite; recreating"
            );
            fs::remove_file(&config.session_file)
                .await
                .with_context(|| {
                    format!(
                        "Failed to remove invalid session file {}",
                        config.session_file
                    )
                })?;
        }

        let Some(session_data_b64) = config.session_data.as_deref() else {
            info!(
                session_file = %config.session_file,
                "Telegram session file not found; a new SQLite session will be created"
            );
            return Ok(());
        };

        info!("Loading Telegram session file from TELEGRAM_SESSION_DATA");
        let session_data = Self::decode_session_base64(session_data_b64)?;

        if Self::looks_like_sqlite_file(&session_data) {
            fs::write(&config.session_file, session_data)
                .await
                .with_context(|| format!("Failed to write session file {}", config.session_file))?;
            return Ok(());
        }

        info!("Detected legacy Telegram session payload; converting to SQLite session format");
        Self::import_legacy_session_data(&config.session_file, &session_data).await?;

        Ok(())
    }

    async fn is_sqlite_file(path: &str) -> bool {
        match fs::read(path).await {
            Ok(bytes) => Self::looks_like_sqlite_file(&bytes),
            Err(_) => false,
        }
    }

    fn looks_like_sqlite_file(bytes: &[u8]) -> bool {
        bytes.starts_with(b"SQLite format 3\0")
    }

    async fn import_legacy_session_data(session_file: &str, session_data: &[u8]) -> Result<()> {
        let legacy_session = LegacySession::load(session_data)
            .context("Failed to parse legacy TELEGRAM_SESSION_DATA payload")?;

        let mut imported = Self::legacy_to_session_data(&legacy_session)?;
        Self::ensure_home_dc_points_to_authorized_dc(&mut imported);

        let sqlite_session = SqliteSession::open(session_file)
            .await
            .with_context(|| format!("Failed to open session file {}", session_file))?;
        imported.import_to(&sqlite_session).await;

        Ok(())
    }

    fn legacy_to_session_data(legacy_session: &LegacySession) -> Result<SessionData> {
        let mut session_data = SessionData::default();
        let mut imported_auth_keys = 0usize;

        for dc_option in session_data.dc_options.values_mut() {
            if let Some(auth_key) = legacy_session.dc_auth_key(dc_option.id) {
                dc_option.auth_key = Some(auth_key);
                imported_auth_keys += 1;
            }
        }

        if let Some(user) = legacy_session.get_user() {
            session_data.home_dc = user.dc;
        }

        if imported_auth_keys == 0 {
            return Err(anyhow::anyhow!(
                "Legacy TELEGRAM_SESSION_DATA did not contain any usable auth keys"
            ));
        }

        Ok(session_data)
    }

    fn ensure_home_dc_points_to_authorized_dc(session_data: &mut SessionData) {
        if session_data
            .dc_options
            .get(&session_data.home_dc)
            .and_then(|dc| dc.auth_key)
            .is_some()
        {
            return;
        }

        if let Some(dc_id) = session_data
            .dc_options
            .iter()
            .find(|(_, dc)| dc.auth_key.is_some())
            .map(|(dc_id, _)| *dc_id)
        {
            session_data.home_dc = dc_id;
        }
    }

    async fn build_connection(config: &RuntimeConfig) -> Result<TelegramConnection> {
        let session = Arc::new(
            SqliteSession::open(&config.session_file)
                .await
                .with_context(|| format!("Failed to open session file {}", config.session_file))?,
        );

        let SenderPool {
            runner,
            updates,
            handle,
        } = SenderPool::new(Arc::clone(&session), config.api_id);

        let pool_handle = handle.thin.clone();
        let client = Client::new(handle);
        let runner_task = tokio::spawn(runner.run());

        Ok(TelegramConnection {
            client,
            updates_rx: Some(updates),
            pool_handle,
            runner_task,
        })
    }

    fn decode_session_base64(session_data_b64: &str) -> Result<Vec<u8>> {
        use base64::{engine::general_purpose::STANDARD, Engine};

        STANDARD
            .decode(session_data_b64.trim())
            .context("Failed to decode TELEGRAM_SESSION_DATA")
    }
}

#[cfg(test)]
mod tests {
    use super::TelegramAuth;
    use grammers_session_legacy::Session as LegacySession;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    #[test]
    fn detects_sqlite_header() {
        assert!(TelegramAuth::looks_like_sqlite_file(
            b"SQLite format 3\0rest"
        ));
        assert!(!TelegramAuth::looks_like_sqlite_file(b"not-sqlite"));
    }

    #[test]
    fn converts_legacy_session_payload() {
        let legacy = LegacySession::new();
        let auth_key = [42u8; 256];
        legacy.insert_dc_tcp(
            2,
            &SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(149, 154, 167, 40), 443)),
            auth_key,
        );
        legacy.set_user(123, 2, false);

        let converted = TelegramAuth::legacy_to_session_data(&legacy).expect("convert legacy");
        assert_eq!(converted.home_dc, 2);
        assert_eq!(
            converted.dc_options.get(&2).and_then(|dc| dc.auth_key),
            Some(auth_key)
        );
    }
}
