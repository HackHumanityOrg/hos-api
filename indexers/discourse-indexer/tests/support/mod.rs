use anyhow::{anyhow, Result};
use async_trait::async_trait;
use discourse_indexer::discourse_api::JsonFetcher;
use discourse_indexer::domain::topics::model::Topic;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub fn fixture_path(file: &str) -> PathBuf {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    base.join("tests").join("fixtures").join(file)
}

pub fn load_fixture(file: &str) -> Result<String> {
    std::fs::read_to_string(fixture_path(file))
        .map_err(|e| anyhow!("failed to read fixture {}: {}", file, e))
}

pub struct FixtureFetcher {
    responses: HashMap<String, Value>,
}

impl FixtureFetcher {
    pub fn new(mapping: &[(&str, &str)]) -> Result<Self> {
        let mut responses = HashMap::with_capacity(mapping.len());
        for (path, fixture) in mapping.iter() {
            let body = load_fixture(fixture)?;
            let value: Value = serde_json::from_str(&body)?;
            responses.insert(path.to_string(), value);
        }
        Ok(Self { responses })
    }
}

#[async_trait]
impl JsonFetcher for FixtureFetcher {
    async fn get_json<T: DeserializeOwned + Send + 'static>(
        &self,
        path: &str,
    ) -> anyhow::Result<T> {
        let value = self
            .responses
            .get(path)
            .ok_or_else(|| anyhow!("missing fixture for path {}", path))?;
        let json = serde_json::to_vec(value)?;
        Ok(serde_json::from_slice::<T>(&json)?)
    }
}

#[allow(dead_code)]
pub fn load_topic_from_fixture(file: &str) -> Result<Topic> {
    let raw = load_fixture(file)?;
    let mut value: Value = serde_json::from_str(&raw)?;
    if value.get("bumped_at").is_none() {
        if let Some(last_posted) = value.get("last_posted_at").cloned() {
            if let Some(obj) = value.as_object_mut() {
                obj.insert("bumped_at".to_string(), last_posted);
            }
        }
    }
    let topic = serde_json::from_value(value)?;
    Ok(topic)
}
