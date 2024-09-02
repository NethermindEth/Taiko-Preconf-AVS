pub mod consensus_layer;
pub mod execution_layer;
pub mod slot_clock;
pub mod validator;

use crate::utils::config::ContractAddresses;
use consensus_layer::ConsensusLayer;
use execution_layer::ExecutionLayer;
use slot_clock::SlotClock;
use std::sync::Arc;

pub struct EthereumL1 {
    pub slot_clock: Arc<SlotClock>,
    pub consensus_layer: ConsensusLayer,
    pub execution_layer: ExecutionLayer,
}

impl EthereumL1 {
    pub async fn new(
        execution_rpc_url: &str,
        avs_node_ecdsa_private_key: &str,
        contract_addresses: &ContractAddresses,
        consensus_rpc_url: &str,
        slot_duration_sec: u64,
        slots_per_epoch: u64,
        preconf_registry_expiry_sec: u64,
    ) -> Result<Self, anyhow::Error> {
        let consensus_layer = ConsensusLayer::new(consensus_rpc_url)?;
        let genesis_details = consensus_layer.get_genesis_details().await?;
        let slot_clock = Arc::new(SlotClock::new(
            0u64,
            genesis_details.genesis_time,
            slot_duration_sec,
            slots_per_epoch,
        ));

        let execution_layer = ExecutionLayer::new(
            execution_rpc_url,
            avs_node_ecdsa_private_key,
            contract_addresses,
            slot_clock.clone(),
            preconf_registry_expiry_sec,
        )
        .await?;

        Ok(Self {
            slot_clock,
            consensus_layer,
            execution_layer,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::node_bindings::Anvil;
    use consensus_layer::tests::setup_server;
    use execution_layer::PreconfTaskManager;

    #[tokio::test]
    async fn test_propose_new_block_with_lookahead() {
        let server = setup_server().await;
        let cl = ConsensusLayer::new(server.url().as_str()).unwrap();
        let _duties = cl.get_lookahead(1).await.unwrap();

        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let private_key = anvil.keys()[0].clone();
        let el = ExecutionLayer::new_from_pk(rpc_url, private_key)
            .await
            .unwrap();

        // TODO:
        // There is a bug in the Anvil (anvil 0.2.0) library:
        // `Result::unwrap()` on an `Err` value: buffer overrun while deserializing
        // check if it's fixed in next version
        // let lookahead_params = el
        //     .get_lookahead_params_for_epoch_using_beacon_lookahead(1, &duties)
        //     .await
        //     .unwrap();
        let lookahead_params = Vec::<PreconfTaskManager::LookaheadSetParam>::new();

        el.propose_new_block(0, vec![0; 32], [0; 32], 0, lookahead_params, true)
            .await
            .unwrap();
    }
}
