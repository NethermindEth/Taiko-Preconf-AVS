use anyhow::Error;
// use beacon_api_client::{mainnet::MainnetClientTypes, Client, GenesisDetails};
// use reqwest;

use alloy::providers::{Provider, ProviderBuilder};
// use alloy::rpc::types::beacon::
use alloy::genesis::Genesis;

pub struct ConsensusLayer<P: Provider> {
    // client: Client<MainnetClientTypes>,
    provider: P,
}

impl<P: Provider> ConsensusLayer<P> {
    pub async fn new(rpc_url: &str) -> Result<Self, Error> {
        // let client = Client::new(reqwest::Url::parse(rpc_url)?);

        // Create an HTTP provider to interact with the beacon node
        let provider = ProviderBuilder::new()
            .on_http(rpc_url.parse()?);
            // .map_err(|e| Error::msg(format!("Failed to create provider: {}", e)))?;

        Ok(Self { provider })
    }

    pub async fn get_genesis_details(&self) -> Result<Genesis, Error> {
        let genesis_data: Genesis = self
            .provider
            .call("eth/v1/beacon/genesis", None::<()>)
            .await
            .map_err(|e| Error::msg(format!("Failed to get genesis details: {}", e)))?;

        Ok(genesis_data)
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
