#![allow(dead_code)] // TODO: remove
use anyhow::Error;
use beacon_api_client::{mainnet::MainnetClientTypes, Client, ProposerDuty};
use reqwest;

pub struct ConsensusLayer {
    client: Client<MainnetClientTypes>,
}

impl ConsensusLayer {
    pub fn new(rpc_url: &str) -> Result<Self, Error> {
        let client = Client::new(reqwest::Url::parse(rpc_url)?);
        Ok(Self { client })
    }

    // First iteration we get the next lookahead by checking the actual epoch number
    // this will be improved so we keep synchronization with the CL
    pub async fn get_latest_lookahead(&self) -> Result<Vec<ProposerDuty>, Error> {
        let header = self.client.get_beacon_header_at_head().await?;
        let slot = header.header.message.slot;
        let epoch = slot / 32;
        self.get_lookeahead(epoch+1).await
    }

    async fn get_lookeahead(&self, epoch: u64) -> Result<Vec<ProposerDuty>, Error> {
        let (_, duties) = self.client.get_proposer_duties(epoch).await?;
        Ok(duties)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio;

    #[tokio::test]
    async fn test_get_lookeahead() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/eth/v1/validator/duties/proposer/1")
            // .match_header("content-type", "application/json")
            .with_body(include_str!("lookahead_test_response.json"))
            .create();
        let cl = ConsensusLayer::new(server.url().as_str()).unwrap();
        let duties = cl.get_lookeahead(1).await.unwrap();

        assert_eq!(duties.len(), 32);
        assert_eq!(duties[0].slot, 32);
    }
}
