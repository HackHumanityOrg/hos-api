pub fn bool_i8(b: bool) -> i8 {
    if b {
        1
    } else {
        0
    }
}

pub fn opt_bool_i8(v: Option<bool>, default: i8) -> i8 {
    v.map(bool_i8).unwrap_or(default)
}

pub fn opt_bool_to_i8(v: Option<bool>) -> Option<i8> {
    v.map(bool_i8)
}

pub fn opt_string(s: Option<String>) -> String {
    s.unwrap_or_default()
}

// Retry a DB operation if it fails due to a deadlock (errno 1213 / 40001),
// with a small linear backoff plus jitter. Retries up to 5 times.
pub async fn with_deadlock_retry<F, Fut, T>(mut op: F) -> Result<T, sea_orm::DbErr>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, sea_orm::DbErr>>,
{
    use rand::Rng;
    let mut attempts = 0u32;
    loop {
        attempts += 1;
        match op().await {
            Ok(v) => return Ok(v),
            Err(e) => {
                let s = e.to_string();
                let is_deadlock = s.contains("Deadlock found")
                    || s.contains("errno 1213")
                    || s.contains("sqlstate 40001");
                if is_deadlock && attempts <= 5 {
                    let jitter: u64 = rand::thread_rng().gen_range(0..25);
                    let delay_ms = 25 * attempts as u64 + jitter;
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                    continue;
                }
                return Err(e);
            }
        }
    }
}
