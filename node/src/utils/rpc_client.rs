use alloy::primitives::B256;
use anyhow::Error;
use http::{HeaderMap, HeaderValue};
use jsonrpsee::{
    core::client::{ClientT, Error as JsonRpcError},
    http_client::{HttpClient, HttpClientBuilder},
};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;

use crate::metrics::Metrics;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    iat: usize,
    exp: usize,
}

const JWT_TOKEN_EXPIRATION_TIME_SECONDS: usize = 3600;

fn create_jwt_token(secret: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    let now: usize = chrono::Utc::now().timestamp().try_into()?;
    let claims = Claims {
        iat: now,
        exp: now + JWT_TOKEN_EXPIRATION_TIME_SECONDS,
    };

    Ok(encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret),
    )?)
}

pub struct JSONRPCClient {
    url: String,
    timeout: Duration,
    jwt_secret: [u8; 32],
    client: RwLock<HttpClient>,
}

impl JSONRPCClient {
    /// Creates a new RpcClient with JWT authentication.
    pub fn new_with_timeout_and_jwt(
        url: &str,
        timeout: Duration,
        jwt_secret: &[u8],
    ) -> Result<Self, Error> {
        if url.is_empty() {
            return Err(anyhow::anyhow!("URL is empty"));
        }

        let jwt_secret: [u8; 32] = jwt_secret
            .try_into()
            .map_err(|e| anyhow::anyhow!("Invalid JWT secret: {e}"))?;
        let client = Self::create_client(url, timeout, &jwt_secret)?;

        Ok(Self {
            url: url.to_string(),
            timeout,
            jwt_secret,
            client: RwLock::new(client),
        })
    }

    fn create_client(
        url: &str,
        timeout: Duration,
        jwt_secret: &[u8; 32],
    ) -> Result<HttpClient, Error> {
        let jwt = B256::from_slice(jwt_secret);

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

        Ok(client)
    }

    pub async fn call_method(&self, method: &str, params: Vec<Value>) -> Result<Value, Error> {
        let result = {
            let client_guard = self.client.read().await;
            client_guard.request(method, params.clone()).await
        };

        match result {
            Ok(result) => Ok(result),
            Err(JsonRpcError::Transport(err)) => {
                if err.to_string().contains("401") {
                    tracing::trace!("401 error, JWT token expired, recreating client");
                    self.recreate_client().await?;
                    return self
                        .client
                        .read()
                        .await
                        .request(method, params)
                        .await
                        .map_err(Error::from);
                }
                Err(anyhow::anyhow!("Http transport error: {err}."))
            }
            Err(err) => Err(Error::from(err)),
        }
    }

    async fn recreate_client(&self) -> Result<(), Error> {
        let new_client = Self::create_client(&self.url, self.timeout, &self.jwt_secret)
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?;

        tracing::trace!("Created new client");
        *self.client.write().await = new_client;
        Ok(())
    }
}

/// A direct HTTP client that doesn't use JSON-RPC
pub struct HttpRPCClient {
    client: RwLock<reqwest::Client>,
    base_url: String,
    timeout: Duration,
    jwt_secret: [u8; 32],
    metrics: Arc<Metrics>,
}

impl HttpRPCClient {
    /// Creates a new DirectHttpClient with JWT authentication
    pub fn new_with_jwt(
        base_url: &str,
        timeout: Duration,
        jwt_secret: &[u8],
        metrics: Arc<Metrics>,
    ) -> Result<Self, Error> {
        if base_url.is_empty() {
            return Err(anyhow::anyhow!("URL is empty"));
        }

        let jwt_secret_bytes: [u8; 32] = jwt_secret
            .try_into()
            .map_err(|e| anyhow::anyhow!("Invalid JWT secret: {e}"))?;

        let client = Self::create_client(timeout, &jwt_secret_bytes)?;

        Ok(Self {
            client: RwLock::new(client),
            base_url: base_url.to_string(),
            timeout,
            jwt_secret: jwt_secret_bytes,
            metrics,
        })
    }

    fn create_client(timeout: Duration, jwt_secret: &[u8; 32]) -> Result<reqwest::Client, Error> {
        let jwt = B256::from_slice(jwt_secret);

        if jwt == B256::ZERO {
            return Err(anyhow::anyhow!("JWT secret is illegal"));
        }

        let jwt_token = create_jwt_token(&jwt.0)
            .map_err(|e| anyhow::anyhow!("Failed to create JWT token: {e}"))?;

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .default_headers({
                let mut headers = HeaderMap::new();
                headers.insert(
                    "authorization",
                    HeaderValue::from_str(&format!("Bearer {}", jwt_token)).map_err(|e| {
                        anyhow::anyhow!("Failed to create header value from jwt token: {e}")
                    })?,
                );
                headers.insert("content-type", HeaderValue::from_static("application/json"));
                headers
            })
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?;

        Ok(client)
    }

    pub async fn retry_request_with_timeout<T>(
        &self,
        method: http::Method,
        endpoint: &str,
        payload: &T,
        max_duration: Duration,
        metric_label: &str,
    ) -> Result<Value, Error>
    where
        T: serde::Serialize,
    {
        let start_time = std::time::Instant::now();
        self.metrics.inc_rpc_driver_call(metric_label);

        // Try until we exceed the max duration
        while start_time.elapsed() < max_duration {
            let response = self.request_json(method.clone(), endpoint, payload).await;

            match response {
                Ok(ref data) => {
                    self.metrics.observe_rpc_driver_call_duration(
                        metric_label,
                        start_time.elapsed().as_secs_f64(),
                    );
                    return Ok(data.clone());
                }
                Err(ref e) => {
                    let elapsed = start_time.elapsed();
                    let remaining = max_duration.checked_sub(elapsed).unwrap_or_default();

                    tracing::error!(
                        "Failed to call driver RPC for API '{}': {}. Retrying... ({}ms elapsed, {}ms remaining)",
                        endpoint,
                        e,
                        elapsed.as_millis(),
                        remaining.as_millis()
                    );

                    tokio::time::sleep(Duration::from_millis(100)).await;
                    if let Err(err) = self.recreate_client().await {
                        self.metrics.inc_rpc_driver_call_error(metric_label);
                        let metric_label_error = format!("{}-error", metric_label);
                        self.metrics.observe_rpc_driver_call_duration(
                            &metric_label_error,
                            start_time.elapsed().as_secs_f64(),
                        );
                        return Err(err);
                    }
                }
            }
        }

        self.metrics.inc_rpc_driver_call_error(metric_label);
        let metric_label_error = format!("{}-error", metric_label);
        self.metrics.observe_rpc_driver_call_duration(
            &metric_label_error,
            start_time.elapsed().as_secs_f64(),
        );

        Err(anyhow::anyhow!(
            "Failed to call driver RPC for API '{}' within the duration ({}ms)",
            endpoint,
            start_time.elapsed().as_millis()
        ))
    }

    /// Send a request to the specified endpoint with the given method and payload
    pub async fn request_json<T: Serialize>(
        &self,
        method: http::Method,
        endpoint: &str,
        payload: &T,
    ) -> Result<Value, Error> {
        let url = format!(
            "{}/{}",
            self.base_url.trim_end_matches('/'),
            endpoint.trim_start_matches('/')
        );

        let mut response = self
            .client
            .read()
            .await
            .request(method.clone(), &url)
            .json(payload)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send HTTP request: {e}"))?;

        if response.status() == http::StatusCode::UNAUTHORIZED {
            tracing::debug!("HttpRPCClient 401 error, recreating client");
            self.recreate_client().await?;
            response = self
                .client
                .read()
                .await
                .request(method, &url)
                .json(payload)
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to send HTTP request: {e}"))?;
        }

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

    pub async fn recreate_client(&self) -> Result<(), Error> {
        let new_client = Self::create_client(self.timeout, &self.jwt_secret)
            .map_err(|e| anyhow::anyhow!("Failed to create HttpRPCClient: {e}"))?;

        tracing::debug!("Created new HttpRPCClient client");
        *self.client.write().await = new_client;
        Ok(())
    }
}
