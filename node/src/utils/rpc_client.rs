use alloy::primitives::B256;
use anyhow::Error;
use http::{HeaderMap, HeaderValue};
use jsonrpsee::core::client::ClientT;
use jsonrpsee::http_client::{HttpClient, HttpClientBuilder};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iat: usize,
}

fn create_jwt_token(secret: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let claims = Claims {
        iat: chrono::Utc::now().timestamp() as usize, // Current timestamp without adding duration
    };

    Ok(encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret),
    )?)
}

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

    /// Creates a new RpcClient with JWT authentication.
    pub fn new_with_timeout_and_jwt(
        url: &str,
        timeout: Duration,
        jwt_secret: &[u8],
    ) -> Result<Self, Error> {
        if url.is_empty() {
            return Err(anyhow::anyhow!("URL is empty"));
        }

        let jwt_secret_bytes: [u8; 32] = jwt_secret
            .try_into()
            .map_err(|e| anyhow::anyhow!("Invalid JWT secret: {e}"))?;
        let jwt = B256::from_slice(&jwt_secret_bytes);

        if jwt == B256::ZERO {
            return Err(anyhow::anyhow!("JWT secret is illegal"));
        }

        let jwt_token = create_jwt_token(&jwt.0)
            .map_err(|e| anyhow::anyhow!("Failed to create JWT token: {e}"))?;

        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_str(&format!("Bearer {}", jwt_token)).map_err(|e| {
                anyhow::anyhow!("Failed to create header value from jwt token: {e}")
            })?,
        );

        let client = HttpClientBuilder::default()
            .request_timeout(timeout)
            .set_headers(headers)
            .build(url)
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?;

        Ok(Self { client })
    }

    pub async fn call_method(&self, method: &str, params: Vec<Value>) -> Result<Value, Error> {
        self.client
            .request(method, params)
            .await
            .map_err(Error::from)
    }
}
