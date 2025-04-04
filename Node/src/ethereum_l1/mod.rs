pub mod consensus_layer;
pub mod execution_layer;
mod l1_contracts_bindings;
mod monitor_transaction;
mod propose_batch_builder;
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

pub struct EthereumL1Config {
    pub execution_ws_rpc_url: String,
    pub avs_node_ecdsa_private_key: String,
    pub contract_addresses: L1ContractAddresses,
    pub consensus_rpc_url: String,
    pub min_priority_fee_per_gas_wei: u64,
    pub tx_fees_increase_percentage: u64,
    pub slot_duration_sec: u64,
    pub slots_per_epoch: u64,
    pub preconf_heartbeat_ms: u64,
    pub max_attempts_to_send_tx: u64,
    pub delay_between_tx_attempts_sec: u64,
}

pub struct EthereumL1 {
    pub slot_clock: Arc<SlotClock>,
    pub _consensus_layer: ConsensusLayer,
    pub execution_layer: ExecutionLayer,
}

impl EthereumL1 {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(config: EthereumL1Config) -> Result<Self, Error> {
        let consensus_layer = ConsensusLayer::new(&config.consensus_rpc_url)?;
        let genesis_details = consensus_layer.get_genesis_details().await?;
        let slot_clock = Arc::new(SlotClock::new(
            0u64,
            genesis_details.genesis_time,
            config.slot_duration_sec,
            config.slots_per_epoch,
            config.preconf_heartbeat_ms,
        ));

        let execution_layer = ExecutionLayer::new(
            &config.execution_ws_rpc_url,
            &config.avs_node_ecdsa_private_key,
            &config.contract_addresses,
            config.min_priority_fee_per_gas_wei,
            config.tx_fees_increase_percentage,
            config.max_attempts_to_send_tx,
            config.delay_between_tx_attempts_sec,
        )
        .await?;

        Ok(Self {
            slot_clock,
            _consensus_layer: consensus_layer,
            execution_layer,
        })
    }
}
