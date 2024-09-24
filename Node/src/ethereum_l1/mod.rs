mod avs_contract_error;
pub mod block_proposed;
pub mod consensus_layer;
mod el_with_cl_tests;
pub mod execution_layer;
pub mod merkle_proofs;
pub mod slot_clock;
mod ws_provider;

use crate::{bls::BLSService, utils::config::ContractAddresses};
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
    pub consensus_layer: ConsensusLayer,
    pub execution_layer: ExecutionLayer,
}

impl EthereumL1 {
    pub async fn new(
        execution_ws_rpc_url: &str,
        avs_node_ecdsa_private_key: &str,
        contract_addresses: &ContractAddresses,
        consensus_rpc_url: &str,
        slot_duration_sec: u64,
        slots_per_epoch: u64,
        preconf_registry_expiry_sec: u64,
        bls_service: Arc<BLSService>,
        l1_chain_id: u64,
    ) -> Result<Self, Error> {
        let consensus_layer = ConsensusLayer::new(consensus_rpc_url)?;
        let genesis_details = consensus_layer.get_genesis_details().await?;
        let slot_clock = Arc::new(SlotClock::new(
            0u64,
            genesis_details.genesis_time,
            slot_duration_sec,
            slots_per_epoch,
        ));

        let execution_layer = ExecutionLayer::new(
            execution_ws_rpc_url,
            avs_node_ecdsa_private_key,
            contract_addresses,
            slot_clock.clone(),
            preconf_registry_expiry_sec,
            bls_service,
            l1_chain_id,
        )
        .await?;

        Ok(Self {
            slot_clock,
            consensus_layer,
            execution_layer,
        })
    }

    pub async fn force_push_lookahead(&self) -> Result<(), Error> {
        // Get next epoch
        let next_epoch = self.slot_clock.get_current_epoch()? + 1;
        // Get CL lookahead for the next epoch
        let cl_lookahead = self.consensus_layer.get_lookahead(next_epoch).await?;
        // Get lookahead params for contract call
        let lookahead_params = self
            .execution_layer
            .get_lookahead_params_for_epoch_using_cl_lookahead(
                self.slot_clock.get_epoch_begin_timestamp(next_epoch)?,
                &cl_lookahead,
            )
            .await?;
        // Force push lookahead to the contract
        self.execution_layer
            .force_push_lookahead(lookahead_params)
            .await?;

        Ok(())
    }
}
