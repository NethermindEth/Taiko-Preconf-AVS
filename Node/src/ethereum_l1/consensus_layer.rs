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
        let genesis_time = genesis["data"]["genesis_time"]
            .as_str()
            .ok_or(anyhow::anyhow!(
                "get_genesis_time error: {}",
                "genesis_time is not a string"
            ))?
            .parse::<u64>()
            .map_err(|err| anyhow::anyhow!("get_genesis_time error: {}", err))?;
        Ok(genesis_time)
    }

    pub async fn get_head_slot_number(&self) -> Result<u64, Error> {
        let headers = self.get("/eth/v1/beacon/headers").await?;

        let slot = headers["data"][0]["header"]["message"]["slot"]
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

    #[tokio::test]
    async fn test_get_head_slot_number() {
        let server = setup_server().await;
        let cl = ConsensusLayer::new(server.url().as_str(), Duration::from_secs(1)).unwrap();
        let slot = cl.get_head_slot_number().await.unwrap();

        assert_eq!(slot, 4269482);
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
            .mock("GET", "/eth/v1/beacon/headers")
            .with_body(r#"
            {"execution_optimistic":false,"finalized":false,"data":[{"root":"0x1394fbcac1b01dc54bbd8ac0e450f9b4c9918aa58eb87a4cd0cd4f9ae454b7e0","canonical":true,"header":{"message":{"slot":"4269482","proposer_index":"589826","parent_root":"0x6dcf6c0fa0dd8e5e0fd50ecd1ccc70885a07d9fab9194043b52d92cce6810fb3","state_root":"0x7134c0ceae868d193ac3627d83a3a135f421377c0ef02bb9eeefe17c9ed5a37e","body_root":"0x3b98639b219e5c00bf0660699f8c6f35e4bf557ffec17ba5d26b3a4a4c1bc028"},"signature":"0x85295d327a6e04e1091475ebe61ed46d231fdb97f8048e305d38b0fd1c2db567b3a8d827e4408ed931518b19d8517eef079ae2c47781a1fbf5e422985f50b21c25ec459dea376703ef1fb0c40a85156c200d639301b27e49f1be377f8dac19e5"}}]}
            "#)
            .create();

        server
    }
}
