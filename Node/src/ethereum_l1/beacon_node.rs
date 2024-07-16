#![allow(dead_code)] // TODO: remove
use anyhow::Error;
use beacon_api_client::{mainnet::MainnetClientTypes, Client, ProposerDuty};
use reqwest;

pub struct BeaconNode {
    rpc_url: reqwest::Url,
}

impl BeaconNode {
    pub fn new(rpc_url: &str) -> Result<Self, Error> {
        let rpc_url = reqwest::Url::parse(rpc_url)?;
        Ok(Self { rpc_url })
    }

    async fn get_lookeahead(&self, epoch: u64) -> Result<Vec<ProposerDuty>, Error> {
        let client: Client<MainnetClientTypes> = Client::new(self.rpc_url.clone());
        let (_, duties) = client.get_proposer_duties(epoch).await?;
        Ok(duties)
    }
}
