use anyhow::Error;
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use serde_json::Value;
use std::time::Duration;
use alloy_primitives::H256;

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

    // pub fn new_with_timeout_and_jwt(
    //     url: &str,
    //     timeout: Duration,
    //     jwt_secret: Option<String>,
    // ) -> Self {
    //     let mut builder = HttpClientBuilder::default().request_timeout(timeout);

    //     if let Some(jwt) = jwt_secret {
    //         builder = builder
    //             .set_header("Authorization", format!("Bearer {}", jwt))
    //             .unwrap();
    //     }

    //     let client = builder.build(url).unwrap();
    //     RpcClient { client }
    // }

    /// Creates a new EngineClient with JWT authentication.
    pub async fn new_jwt(url: &str, jwt_secret: &str) -> Result<Self, Box<dyn std::error::Error>> {
        // Convert JWT secret to bytes32
        let jwt = H256::from_str(jwt_secret).map_err(|_| "Invalid JWT secret")?;

        if jwt == H256::ZERO || url.is_empty() {  // Note: ZERO instead of zero()
            return Err("URL is empty or JWT secret is illegal".into());
        }

        // Create JWT token
        let jwt_token = create_jwt_token(&jwt.0)?;

        // Configure HTTP client with JWT authentication
        let client = HttpClientBuilder::default()
            .set_headers(
                [(
                    "Authorization",
                    format!("Bearer {}", jwt_token).as_str(),
                )]
                .into_iter()
                .collect(),
            )
            .build(url)?;

        Ok(EngineClient { client })
    }

    pub async fn call_method(&self, method: &str, params: Vec<Value>) -> Result<Value, Error> {
        self.client
            .request(method, params)
            .await
            .map_err(Error::from)
    }
}
