use anyhow::Error;
use beacon_api_client::{mainnet::MainnetClientTypes, Client, GenesisDetails};
use reqwest;

pub struct ConsensusLayer {
    client: Client<MainnetClientTypes>,
}

impl ConsensusLayer {
    pub fn new(rpc_url: &str) -> Result<Self, Error> {
        let client = Client::new(reqwest::Url::parse(rpc_url)?);
        Ok(Self { client })
    }

    pub async fn get_genesis_details(&self) -> Result<GenesisDetails, Error> {
        tracing::debug!("Getting genesis details");
        self.client.get_genesis_details().await.map_err(Error::new)
    }

    pub async fn get_head_slot_number(&self) -> Result<u64, Error> {
        let result = self
            .client
            .get_beacon_header(beacon_api_client::BlockId::Head)
            .await
            .map_err(|err| anyhow::anyhow!("get_head_slot_number error: {}", err))?;
        tracing::debug!(
            "Head slot number from beacon: {}",
            result.header.message.slot
        );
        Ok(result.header.message.slot)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_get_genesis_data() {
        let server = setup_server().await;
        let cl = ConsensusLayer::new(server.url().as_str()).unwrap();
        let genesis_data = cl.get_genesis_details().await.unwrap();

        assert_eq!(genesis_data.genesis_time, 1590832934);
        assert_eq!(
            genesis_data.genesis_validators_root.to_string(),
            "0xcf8e0d4e9587369b2301d0790347320302cc0943d5a1884560367e8208d920f2"
        );
        assert_eq!(genesis_data.genesis_fork_version, [0; 4]);
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
