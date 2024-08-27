use crate::ethereum_l1::EthereumL1;
use crate::utils::rpc_client::RpcClient;
use anyhow::Error;
use std::sync::Arc;

pub mod constraints;
use constraints::{Constraint, ConstraintsMessage, SignedConstraints};

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
        constraints: Vec<Constraint>,
        ethereum_l1: Arc<EthereumL1>,
    ) -> Result<(), Error> {
        // Prepare the message
        // TODO check slot id value
        let slot_id = ethereum_l1.slot_clock.get_current_slot()?;

        let message = ConstraintsMessage::new(self.validator_index, slot_id, constraints);

        let data_to_sign: Vec<u8> = message.clone().into();

        // Sign the message
        // TODO: Determine if the transaction data needs to be signed as a JSON string.
        let signature = ethereum_l1
            .execution_layer
            .sign_message_with_private_ecdsa_key(&data_to_sign)?;

        // Prepare data to send
        let signed_constraints =
            SignedConstraints::new(message, format!("0x{}", hex::encode(signature)));
        let json_data = serde_json::to_value(&signed_constraints).unwrap();

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
