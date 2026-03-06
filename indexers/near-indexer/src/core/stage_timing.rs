use std::collections::BTreeMap;
use std::future::Future;
use std::time::Instant;

use tracing::info;

#[derive(Debug, Default, Clone)]
pub struct StageMetrics {
    phase_ms: BTreeMap<String, u64>,
    counts: BTreeMap<String, u64>,
    flags: BTreeMap<String, bool>,
}

impl StageMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn time_future<S, T, Fut>(&mut self, name: S, fut: Fut) -> T
    where
        S: Into<String>,
        Fut: Future<Output = T>,
    {
        let name = name.into();
        let start = Instant::now();
        let output = fut.await;
        self.record_phase_elapsed(name, start);
        output
    }

    pub fn record_phase_elapsed<S>(&mut self, name: S, start: Instant)
    where
        S: Into<String>,
    {
        self.record_phase_ms(name, elapsed_ms(start));
    }

    pub fn record_phase_ms<S>(&mut self, name: S, value: u64)
    where
        S: Into<String>,
    {
        self.phase_ms.insert(name.into(), value);
    }

    pub fn record_count<S, V>(&mut self, name: S, value: V)
    where
        S: Into<String>,
        V: TryInto<u64>,
    {
        let value = value.try_into().unwrap_or(u64::MAX);
        self.counts.insert(name.into(), value);
    }

    pub fn record_flag<S>(&mut self, name: S, value: bool)
    where
        S: Into<String>,
    {
        self.flags.insert(name.into(), value);
    }

    pub fn merge_with_prefix(&mut self, prefix: &str, other: StageMetrics) {
        for (name, value) in other.phase_ms {
            self.phase_ms.insert(format!("{prefix}{name}"), value);
        }
        for (name, value) in other.counts {
            self.counts.insert(format!("{prefix}{name}"), value);
        }
        for (name, value) in other.flags {
            self.flags.insert(format!("{prefix}{name}"), value);
        }
    }

    pub fn phase_ms(&self) -> &BTreeMap<String, u64> {
        &self.phase_ms
    }

    pub fn counts(&self) -> &BTreeMap<String, u64> {
        &self.counts
    }

    pub fn flags(&self) -> &BTreeMap<String, bool> {
        &self.flags
    }
}

pub struct BlockProcessingLog<'a> {
    module: &'a str,
    height: u64,
    shard_count: usize,
    started_at: Instant,
    metrics: StageMetrics,
}

impl<'a> BlockProcessingLog<'a> {
    pub fn new(module: &'a str, height: u64, shard_count: usize) -> Self {
        Self {
            module,
            height,
            shard_count,
            started_at: Instant::now(),
            metrics: StageMetrics::new(),
        }
    }

    pub fn metrics_mut(&mut self) -> &mut StageMetrics {
        &mut self.metrics
    }

    pub fn log_success(&self, message: &'static str) {
        let elapsed_ms = elapsed_ms(self.started_at);
        match (
            self.metrics.counts().is_empty(),
            self.metrics.flags().is_empty(),
        ) {
            (true, true) => info!(
                module = self.module,
                height = self.height,
                shard_count = self.shard_count,
                elapsed_ms = elapsed_ms,
                phase_ms = ?self.metrics.phase_ms(),
                "{message}"
            ),
            (false, true) => info!(
                module = self.module,
                height = self.height,
                shard_count = self.shard_count,
                elapsed_ms = elapsed_ms,
                phase_ms = ?self.metrics.phase_ms(),
                counts = ?self.metrics.counts(),
                "{message}"
            ),
            (true, false) => info!(
                module = self.module,
                height = self.height,
                shard_count = self.shard_count,
                elapsed_ms = elapsed_ms,
                phase_ms = ?self.metrics.phase_ms(),
                flags = ?self.metrics.flags(),
                "{message}"
            ),
            (false, false) => info!(
                module = self.module,
                height = self.height,
                shard_count = self.shard_count,
                elapsed_ms = elapsed_ms,
                phase_ms = ?self.metrics.phase_ms(),
                counts = ?self.metrics.counts(),
                flags = ?self.metrics.flags(),
                "{message}"
            ),
        }
    }
}

fn elapsed_ms(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::StageMetrics;

    #[test]
    fn merge_with_prefix_applies_to_all_metric_types() {
        let mut left = StageMetrics::new();
        let mut right = StageMetrics::new();

        right.record_phase_ms("store", 42);
        right.record_count("rows", 7_u64);
        right.record_flag("refresh_triggered", true);

        left.merge_with_prefix("projection.", right);

        assert_eq!(left.phase_ms().get("projection.store"), Some(&42));
        assert_eq!(left.counts().get("projection.rows"), Some(&7));
        assert_eq!(
            left.flags().get("projection.refresh_triggered"),
            Some(&true)
        );
    }
}
