use crate::bls::BLSService;
use anyhow::Error;
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;

pub mod constraints;
use constraints::{ConstraintsMessage, SignedConstraints};

pub struct MevBoost {
    url: String,
    genesis_fork_version: [u8; 4],
}

impl MevBoost {
    pub fn new(url: &str, genesis_fork_version: [u8; 4]) -> Self {
        Self {
            url: url.to_string(),
            genesis_fork_version,
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
        let pubkey = bls_service.get_public_key();
        let message = ConstraintsMessage::new(pubkey, slot_id, constraints);

        let signed = SignedConstraints::new(message, bls_service, self.genesis_fork_version)?;

        let json_data = serde_json::to_value([&signed])?;

        let res = self.post_constraints(json_data).await?;
        tracing::debug!("MEV Boost response status: {}", res);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mev_boost() {
        let constraints = vec![vec![
            2, 249, 3, 213, 131, 48, 24, 36, 6, 132, 59, 154, 202, 0, 133, 4, 168, 23, 200, 0, 131,
            15, 66, 64, 148, 96, 100, 247, 86, 247, 243, 220, 130, 128, 193, 207, 160, 28, 228, 26,
            55, 181, 241, 109, 241, 128, 185, 3, 100, 242, 118, 107, 125, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 128, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 96, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 28, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 3, 64, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 1, 192, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 97, 69, 97,
            210, 209, 67, 98, 30, 18, 110, 135, 131, 26, 239, 40, 118, 120, 180, 66, 184, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 1, 96, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 1, 128, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 188, 101, 74, 119, 65, 102, 81, 67, 67, 47, 47, 104, 55, 117, 72,
            107, 67, 43, 72, 97, 68, 65, 111, 120, 89, 65, 89, 82, 51, 78, 90, 81, 65, 104, 81, 98,
            56, 73, 54, 119, 65, 103, 119, 77, 48, 85, 74, 82, 83, 107, 97, 85, 53, 70, 48, 101,
            70, 43, 116, 121, 84, 55, 47, 54, 99, 110, 79, 116, 54, 86, 72, 71, 97, 53, 73, 99, 86,
            85, 80, 102, 99, 112, 119, 65, 65, 103, 77, 65, 66, 111, 75, 114, 75, 88, 82, 69, 106,
            104, 98, 106, 115, 115, 104, 112, 50, 119, 104, 83, 102, 54, 47, 117, 79, 101, 70, 110,
            69, 82, 56, 113, 57, 52, 99, 80, 119, 103, 120, 116, 108, 51, 110, 79, 78, 111, 67, 82,
            115, 53, 98, 70, 52, 108, 87, 85, 113, 101, 118, 122, 121, 109, 106, 50, 75, 81, 71,
            74, 106, 57, 117, 87, 54, 107, 110, 74, 101, 85, 86, 68, 112, 47, 81, 43, 83, 65, 49,
            78, 74, 65, 81, 65, 65, 47, 47, 56, 97, 57, 84, 57, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 192, 1,
            160, 175, 154, 114, 57, 217, 247, 104, 167, 114, 56, 70, 166, 250, 175, 140, 155, 4,
            255, 254, 185, 177, 119, 184, 80, 186, 72, 127, 32, 38, 28, 186, 156, 160, 63, 77, 214,
            119, 203, 52, 47, 233, 31, 158, 227, 102, 147, 186, 40, 117, 153, 98, 44, 15, 194, 109,
            222, 72, 181, 48, 135, 170, 91, 85, 51, 123,
        ]];
        let slot_id = 11;
        let genesis_fork_version = [16, 0, 0, 56];
        let bls_service = Arc::new(
            BLSService::new("108e47e28c6c6027eac478d742cbb5ef675e20fdb6cbfcbf2bf0cd725b649813")
                .unwrap(),
        );
        let pubkey = bls_service.get_public_key();
        let message = ConstraintsMessage::new(pubkey, slot_id, constraints);
        let signed = SignedConstraints::new(message, bls_service, genesis_fork_version).unwrap();

        let json_data = serde_json::to_value([&signed]).unwrap();

        assert_eq!(json_data[0]["message"]["pubkey"], "0x908d6f98b5eaf6ac1b632c6b80b304612d48afd9c104874f9025960accdae128028119608b0d95a7e141390101fba669");
        assert_eq!(json_data[0]["message"]["slot"], 11);
        assert_eq!(json_data[0]["signature"],"0x952d6076a21d43069b883b84e015fb3f56d4333b069bda1a120243e020d560b5f0b4af297d32e2cba69e9c67bb4a320a0e980df34b3934c4554a6fb32e3490d568d885217b4d5657fc58cefb30f30209f40c0223480c897600313a650fa29ab2");
    }
}
