pub mod consensus_layer;
pub mod execution_layer;
pub mod slot_clock;

use consensus_layer::ConsensusLayer;
use execution_layer::ExecutionLayer;
use slot_clock::SlotClock;
use std::rc::Rc;

pub struct EthereumL1 {
    pub slot_clock: Rc<SlotClock>,
    pub consensus_layer: ConsensusLayer,
    pub execution_layer: ExecutionLayer,
}

impl EthereumL1 {
    pub async fn new(
        execution_rpc_url: &str,
        private_key: &str,
        taiko_preconfirming_address: &str,
        consensus_rpc_url: &str,
        slot_duration_sec: u64,
        slots_per_epoch: u64,
    ) -> Result<Self, anyhow::Error> {
        let consensus_layer = ConsensusLayer::new(consensus_rpc_url)?;
        let genesis_details = consensus_layer.get_genesis_details().await?;
        let slot_clock = Rc::new(SlotClock::new(
            0u64,
            genesis_details.genesis_time,
            slot_duration_sec,
            slots_per_epoch,
        ));

        let execution_layer = ExecutionLayer::new(
            execution_rpc_url,
            private_key,
            taiko_preconfirming_address,
            slot_clock.clone(),
        )?;

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

    #[tokio::test]
    async fn test_propose_new_block_with_lookahead() {
        let server = setup_server().await;
        let cl = ConsensusLayer::new(server.url().as_str()).unwrap();
        let duties = cl.get_lookahead(1).await.unwrap();

        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let private_key = anvil.keys()[0].clone();
        let el = ExecutionLayer::new_from_pk(rpc_url, private_key).unwrap();

        el.propose_new_block(vec![0; 32], [0; 32], duties)
            .await
            .unwrap();
    }
}
