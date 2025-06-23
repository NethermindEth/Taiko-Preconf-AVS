use anyhow::Error;
use std::time::Duration;

pub struct Web3Signer {
    client: reqwest::Client,
    url: reqwest::Url,
}

impl Web3Signer {
    pub fn new(rpc_url: &str, timeout: Duration) -> Result<Self, Error> {
        let client = reqwest::Client::builder().timeout(timeout).build()?;
        Ok(Self {
            client,
            url: reqwest::Url::parse(rpc_url)?,
        })
    }
}
