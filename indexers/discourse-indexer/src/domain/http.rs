use anyhow::Result;
use serde::de::DeserializeOwned;

use crate::discourse_api::DiscourseClient;

pub trait JsonFetcher {
    fn get_json<'a, T: DeserializeOwned + 'a>(
        &'a self,
        path: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>>;
}

impl JsonFetcher for DiscourseClient {
    fn get_json<'a, T: DeserializeOwned + 'a>(
        &'a self,
        path: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>> {
        Box::pin(async move { self.get_json::<T>(path).await })
    }
}
