use crate::utils::rpc_client::RpcClient;
use crate::{bls::BLSService, ethereum_l1::EthereumL1};
use anyhow::Error;
use std::sync::Arc;

pub mod constraints;
use constraints::{ConstraintsMessage, SignedConstraints};

pub struct MevBoost {
    rpc_client: RpcClient,
    validator_index: u64,
}

impl MevBoost {
    pub fn new(rpc_url: &str, validator_index: u64) -> Self {
        let rpc_client = RpcClient::new(rpc_url);
        Self {
            rpc_client,
            validator_index,
        }
    }

    pub async fn force_inclusion(
        &self,
        constraints: Vec<Vec<u8>>,
        ethereum_l1: Arc<EthereumL1>,
        bls_service: Arc<BLSService>,
    ) -> Result<(), Error> {
        // Prepare the message
        // TODO check slot id value
        let slot_id = ethereum_l1.slot_clock.get_current_slot()?;

        let message = ConstraintsMessage::new(self.validator_index, slot_id, false, constraints);

        let signed = SignedConstraints::new(message, bls_service);

        let json_data = serde_json::to_value(&signed)?;

        // https://chainbound.github.io/bolt-docs/api/builder#ethv1builderconstraints
        let method = "/eth/v1/builder/constraints";
        // Make rpc request
        self.rpc_client
            .call_method(method, vec![json_data])
            .await
            .unwrap();

        Ok(())
    }
}
