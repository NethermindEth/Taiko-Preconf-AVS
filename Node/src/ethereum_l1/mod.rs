pub mod consensus_layer;
pub mod execution_layer;

use consensus_layer::ConsensusLayer;
use execution_layer::ExecutionLayer;

pub struct EthereumL1 {
    pub consensus_layer: ConsensusLayer,
    pub execution_layer: ExecutionLayer,
}

impl EthereumL1 {
    pub fn new(
        execution_rpc_url: &str,
        private_key: &str,
        taiko_preconfirming_address: &str,
        consensus_rpc_url: &str,
    ) -> Result<Self, anyhow::Error> {
        let consensus_layer = ConsensusLayer::new(consensus_rpc_url)?;
        let execution_layer =
            ExecutionLayer::new(execution_rpc_url, private_key, taiko_preconfirming_address)?;
        Ok(Self {
            consensus_layer,
            execution_layer,
        })
    }
}
