use crate::bls::BLSService;
use anyhow::Error;
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;

pub mod constraints;
use constraints::{ConstraintsMessage, SignedConstraints};

pub struct MevBoost {
    url: String,
}

impl MevBoost {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }

    async fn post_constraints(&self, params: Value) -> Result<u16, Error> {
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

        Ok(response.status().as_u16())
    }

    pub async fn force_inclusion(
        &self,
        constraints: Vec<Vec<u8>>,
        slot_id: u64,
        bls_service: Arc<BLSService>,
    ) -> Result<(), Error> {
        // Prepare the message
        let pubkey = bls_service.get_ethereum_public_key();
        let message = ConstraintsMessage::new(pubkey, slot_id, constraints);

        let signed = SignedConstraints::new(message, bls_service);

        let json_data = serde_json::to_value([&signed])?;

        let res = self.post_constraints(json_data).await?;
        tracing::debug!("MEV Boost response status: {}", res);

        Ok(())
    }
}
