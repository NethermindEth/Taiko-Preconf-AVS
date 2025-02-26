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

pub struct JSONRPCClient {
    client: HttpClient,
}

impl JSONRPCClient {
    pub fn new(url: &str) -> Result<Self, Error> {
        Self::new_with_timeout(url, Duration::from_secs(10))
    }

    pub fn new_with_timeout(url: &str, timeout: Duration) -> Result<Self, Error> {
        let client = HttpClientBuilder::default()
            .request_timeout(timeout)
            .build(url)
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?;
        Ok(JSONRPCClient { client })
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
            .map_err(|e| anyhow::anyhow!("Failed to create authenticated HTTP client: {e}"))?;

        Ok(Self { client })
    }

    pub async fn call_method(&self, method: &str, params: Vec<Value>) -> Result<Value, Error> {
        self.client
            .request(method, params)
            .await
            .map_err(Error::from)
    }
}

/// A direct HTTP client that doesn't use JSON-RPC
pub struct HttpRPCClient {
    client: reqwest::Client,
    base_url: String,
}

impl HttpRPCClient {
    /// Creates a new DirectHttpClient with JWT authentication
    pub fn new_with_jwt(
        base_url: &str,
        timeout: Duration,
        jwt_secret: &[u8],
    ) -> Result<Self, Error> {
        if base_url.is_empty() {
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

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .default_headers({
                let mut headers = HeaderMap::new();
                // TODO: uncomment, use jwt token
                // headers.insert(
                //     "authorization",
                //     HeaderValue::from_str(&format!("Bearer {}", jwt_token)).map_err(|e| {
                //         anyhow::anyhow!("Failed to create header value from jwt token: {e}")
                //     })?,
                // );
                headers.insert("content-type", HeaderValue::from_static("application/json"));
                headers
            })
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?;

        Ok(Self {
            client,
            base_url: base_url.to_string(),
        })
    }

    /// Send a POST request to the specified endpoint with the given payload
    pub async fn post_json<T: Serialize>(
        &self,
        endpoint: &str,
        payload: &T,
    ) -> Result<Value, Error> {
        let url = if self.base_url.ends_with('/') || endpoint.starts_with('/') {
            format!("{}{}", self.base_url, endpoint.trim_start_matches('/'))
        } else {
            format!("{}/{}", self.base_url, endpoint)
        };

        let response = self
            .client
            .post(&url)
            .json(payload)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send HTTP request: {e}"))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "HTTP request failed with status: {}, body: {}",
                response.status(),
                response.text().await.unwrap_or_default()
            ));
        }

        response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse response as JSON: {e}"))
    }
}
