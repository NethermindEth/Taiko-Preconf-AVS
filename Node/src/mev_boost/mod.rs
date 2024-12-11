use crate::bls::BLSService;
use anyhow::Error;
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;

pub mod constraints;
use constraints::{ConstraintsMessage, SignedConstraints};

mod tests;

pub struct MevBoost {
    url: String,
}

impl MevBoost {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }

    async fn post_constraints(&self, params: Value) -> Result<Value, Error> {
        let client = Client::new();
        // Send the POST request to the MEV Boost
        let response = client
            .post(self.url.clone() + "/constraints/v1/builder/constraints")
            .json(&params)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("MEV Boost failed to send message: {}", e))?;

        // Check the response status
        if !response.status().is_success() {
            return Err(anyhow::anyhow!(
                "MEV Boost received non-success status: {}",
                response.status()
            ));
        }

        // Attempt to parse the response as JSON
        let json: Value = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("MEV Boost failed to parse JSON: {}", e))?;

        Ok(json)
    }

    pub async fn force_inclusion(
        &self,
        constraints: Vec<Vec<u8>>,
        slot_id: u64,
        bls_service: Arc<BLSService>,
    ) -> Result<(), Error> {
        // Prepare the message
        let pubkey: [u8; 48] = bls_service
            .get_public_key_compressed()
            .try_into()
            .map_err(|e| anyhow::anyhow!("BLS service failed to get public key: {:?}", e))?;
        let message = ConstraintsMessage::new(pubkey, slot_id, constraints);

        let signed = SignedConstraints::new(message, bls_service);

        let json_data = serde_json::to_value([&signed])?;

        let res = self.post_constraints(json_data).await?;
        tracing::debug!("MEV Boost response: {:?}", res);

        Ok(())
    }
}
