use crate::{ethereum_l1::EthereumL1, utils::types::*};
use anyhow::Error;
use std::sync::Arc;

pub struct Operator {
    ethereum_l1: Arc<EthereumL1>,
    epoch_begin_timestamp: u64,
    lookahead_required_contract_called: bool,
    lookahead_preconfer_addresses: Vec<PreconferAddress>,
    l1_slots_per_epoch: u64,
}

pub enum Status {
    None,
    Preconfer,
    PreconferAndProposer, // has to force include transactions
}

impl Operator {
    pub fn new(ethereum_l1: Arc<EthereumL1>, epoch: Epoch) -> Result<Self, Error> {
        let l1_slots_per_epoch = ethereum_l1.slot_clock.get_slots_per_epoch();
        let epoch_begin_timestamp = ethereum_l1.slot_clock.get_epoch_begin_timestamp(epoch)?;
        Ok(Self {
            ethereum_l1,
            epoch_begin_timestamp,
            lookahead_required_contract_called: false,
            lookahead_preconfer_addresses: vec![],
            l1_slots_per_epoch,
        })
    }

    pub async fn get_status(&mut self, slot: Slot) -> Result<Status, Error> {
        if self.lookahead_preconfer_addresses.len() < self.l1_slots_per_epoch as usize {
            return Err(anyhow::anyhow!(
                "Operator::get_status: Not enough lookahead params"
            ));
        }

        let slot = slot % self.l1_slots_per_epoch;

        // If the preconfer address is zero, next epoch preconfer may start preconfirming.
        // Update the lookahead to check if it is assigned as a preconfer for the rest slots
        // of the current epoch.
        if self.lookahead_preconfer_addresses[slot as usize] == PRECONFER_ADDRESS_ZERO {
            self.update_preconfer_lookahead_for_epoch().await?;
        }

        if self.lookahead_preconfer_addresses[slot as usize]
            == self.ethereum_l1.execution_layer.get_preconfer_address()
        {
            if self.is_the_final_slot_to_preconf(slot) {
                return Ok(Status::PreconferAndProposer);
            }
            return Ok(Status::Preconfer);
        }

        Ok(Status::None)
    }

    fn is_the_final_slot_to_preconf(&self, slot_mod_slots_per_epoch: Slot) -> bool {
        slot_mod_slots_per_epoch == self.l1_slots_per_epoch - 1
            || self.lookahead_preconfer_addresses[(slot_mod_slots_per_epoch + 1) as usize]
                != self.ethereum_l1.execution_layer.get_preconfer_address()
    }

    pub async fn should_post_lookahead(&mut self) -> Result<bool, Error> {
        if !self.lookahead_required_contract_called {
            self.lookahead_required_contract_called = true;
            if self
                .ethereum_l1
                .execution_layer
                .is_lookahead_required(self.epoch_begin_timestamp)
                .await?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub async fn update_preconfer_lookahead_for_epoch(&mut self) -> Result<(), Error> {
        self.lookahead_preconfer_addresses = self
            .ethereum_l1
            .execution_layer
            .get_lookahead_preconfer_addresses_for_epoch(self.epoch_begin_timestamp)
            .await?;
        Ok(())
    }
}
