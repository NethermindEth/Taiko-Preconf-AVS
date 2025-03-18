pub mod consensus_layer;
pub mod execution_layer;
mod l1_contracts_bindings;
pub mod slot_clock;
mod ws_provider;

use crate::utils::config::L1ContractAddresses;
use anyhow::Error;
use consensus_layer::ConsensusLayer;
#[cfg(not(test))]
use execution_layer::ExecutionLayer;
#[cfg(test)]
#[cfg_attr(feature = "use_mock", double)]
use execution_layer::ExecutionLayer;
#[cfg(test)]
#[cfg(feature = "use_mock")]
use mockall_double::double;
use slot_clock::SlotClock;
use std::sync::Arc;

pub struct EthereumL1 {
    pub slot_clock: Arc<SlotClock>,
    pub _consensus_layer: ConsensusLayer,
    pub execution_layer: ExecutionLayer,
}

impl EthereumL1 {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        execution_ws_rpc_url: &str,
        avs_node_ecdsa_private_key: &str,
        contract_addresses: &L1ContractAddresses,
        consensus_rpc_url: &str,
        slot_duration_sec: u64,
        slots_per_epoch: u64,
        preconf_heartbeat_ms: u64,
    ) -> Result<Self, Error> {
        let consensus_layer = ConsensusLayer::new(consensus_rpc_url)?;
        let genesis_details = consensus_layer.get_genesis_details().await?;
        let slot_clock = Arc::new(SlotClock::new(
            0u64,
            genesis_details.genesis_time,
            slot_duration_sec,
            slots_per_epoch,
            preconf_heartbeat_ms,
        ));

        let execution_layer = ExecutionLayer::new(
            execution_ws_rpc_url,
            avs_node_ecdsa_private_key,
            contract_addresses,
        )
        .await?;

        Ok(Self {
            slot_clock,
            _consensus_layer: consensus_layer,
            execution_layer,
        })
    }
}
