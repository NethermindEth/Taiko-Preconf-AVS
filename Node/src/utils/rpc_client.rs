use anyhow::Error;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use serde_json::Value;
use std::time::Duration;

pub struct RpcClient {
    client: HttpClient,
}

impl RpcClient {
    pub fn new(url: &str) -> Self {
        Self::new_with_timeout(url, Duration::from_secs(10))
    }

    pub fn new_with_timeout(url: &str, timeout: Duration) -> Self {
        let client = HttpClientBuilder::default()
            .request_timeout(timeout)
            .build(url)
            .unwrap();
        RpcClient { client }
    }

    pub async fn call_method(&self, method: &str, params: Vec<Value>) -> Result<Value, Error> {
        self.client
            .request(method, params)
            .await
            .map_err(Error::from)
    }
}
