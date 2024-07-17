pub mod consensus_layer;
pub mod execution_layer;

use consensus_layer::ConsensusLayer;
use execution_layer::ExecutionLayer;

pub struct EthereumL1 {
    pub consensus_layer: ConsensusLayer,
    pub execution_layer: ExecutionLayer,
}

impl EthereumL1 {
    pub async fn new(
        execution_rpc_url: &str,
        private_key: &str,
        taiko_preconfirming_address: &str,
        consensus_rpc_url: &str,
    ) -> Result<Self, anyhow::Error> {
        let consensus_layer = ConsensusLayer::new(consensus_rpc_url)?;
        let genesis_data = consensus_layer.get_genesis_data().await?;
        let execution_layer = ExecutionLayer::new(
            execution_rpc_url,
            private_key,
            taiko_preconfirming_address,
            genesis_data.genesis_time,
        )?;
        Ok(Self {
            consensus_layer,
            execution_layer,
        })
    }
}
