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
    validator_index: u64,
}

impl MevBoost {
    pub fn new(url: &str, validator_index: u64) -> Self {
        Self {
            url: url.to_string(),
            validator_index,
        }
    }

    async fn post_constraints(&self, params: Value) -> Result<Value, Error> {
        let client = Client::new();
        let response = client
            .post(self.url.clone())
            .json(&params)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send message: {}", e))?;

        let json: Value = response.json().await.unwrap();
        Ok(json)
    }

    pub async fn force_inclusion(
        &self,
        constraints: Vec<Vec<u8>>,
        slot_id: u64,
        bls_service: Arc<BLSService>,
    ) -> Result<(), Error> {
        // Prepare the message

        let message = ConstraintsMessage::new(self.validator_index, slot_id, constraints);

        let signed = SignedConstraints::new(message, bls_service);

        let json_data = serde_json::to_value([&signed])?;

        self.post_constraints(json_data).await?;

        Ok(())
    }
}
