use std::time::Duration;

use anyhow::Error;
use reqwest;

pub struct ConsensusLayer {
    client: reqwest::Client,
    url: reqwest::Url,
}

impl ConsensusLayer {
    pub fn new(rpc_url: &str, timeout: Duration) -> Result<Self, Error> {
        let client = reqwest::Client::builder().timeout(timeout).build()?;
        Ok(Self {
            client,
            url: reqwest::Url::parse(rpc_url)?,
        })
    }

    pub async fn get_genesis_time(&self) -> Result<u64, Error> {
        tracing::debug!("Getting genesis time");
        let genesis = self.get("/eth/v1/beacon/genesis").await?;
        let genesis_time = genesis.get("data")
            .and_then(|data| data.get("genesis_time"))
            .and_then(|genesis_time| genesis_time.as_str())
            .ok_or_else(|| anyhow::anyhow!(
                "get_genesis_time error: missing or invalid 'genesis_time' field"
            ))?
            .parse::<u64>()
            .map_err(|err| anyhow::anyhow!("get_genesis_time error: {}", err))?;
        Ok(genesis_time)
    }

    pub async fn get_head_slot_number(&self) -> Result<u64, Error> {
        let headers = self.get("/eth/v1/beacon/headers").await?;

        let slot = headers["data"]["header"]["message"]["slot"]
            .as_str()
            .ok_or(anyhow::anyhow!(
                "get_head_slot_number error: {}",
                "slot is not a string"
            ))?
            .parse::<u64>()
            .map_err(|err| anyhow::anyhow!("get_head_slot_number error: {}", err))?;
        Ok(slot)
    }

    async fn get(&self, path: &str) -> Result<serde_json::Value, Error> {
        let response = self.client.get(self.url.join(path)?).send().await?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "Request ({}) failed with status: {}",
                path,
                response.status()
            ));
        }

        let body = response.text().await?;
        let v: serde_json::Value = serde_json::from_str(&body)?;
        Ok(v)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_get_genesis_data() {
        let server = setup_server().await;
        let cl = ConsensusLayer::new(server.url().as_str(), Duration::from_secs(1)).unwrap();
        let genesis_time = cl.get_genesis_time().await.unwrap();

        assert_eq!(genesis_time, 1590832934);
    }

    pub async fn setup_server() -> mockito::ServerGuard {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/eth/v1/beacon/genesis")
            .with_body(r#"{
                "data": {
                  "genesis_time": "1590832934",
                  "genesis_validators_root": "0xcf8e0d4e9587369b2301d0790347320302cc0943d5a1884560367e8208d920f2",
                  "genesis_fork_version": "0x00000000"
                }
              }"#)
            .create();
        server
            .mock("GET", "/eth/v1/validator/duties/proposer/1")
            .with_body(include_str!("lookahead_test_response.json"))
            .create();
        server
    }
}
