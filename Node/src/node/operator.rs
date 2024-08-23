use crate::{
    ethereum_l1::{execution_layer::LookaheadSetParam, slot_clock::Slot, EthereumL1},
    utils::types::*,
};
use anyhow::Error;
use beacon_api_client::ProposerDuty;
use std::sync::Arc;

pub struct Operator {
    ethereum_l1: Arc<EthereumL1>,
    should_post_lookahead_for_next_epoch: bool,
    lookahead_params: Vec<LookaheadSetParam>,
    l1_slots_per_epoch: u64,
}

pub enum Status {
    None,
    Preconfer,
    PreconferAndProposer, // has to force include transactions
}

impl Operator {
    pub fn new(ethereum_l1: Arc<EthereumL1>) -> Self {
        let l1_slots_per_epoch = ethereum_l1.slot_clock.get_slots_per_epoch();
        Self {
            ethereum_l1,
            should_post_lookahead_for_next_epoch: false,
            lookahead_params: vec![],
            l1_slots_per_epoch,
        }
    }

    pub fn get_status(&self, slot: Slot) -> Result<Status, Error> {
        if self.lookahead_params.len() < self.l1_slots_per_epoch as usize {
            return Err(anyhow::anyhow!(
                "Operator::get_status: Not enough lookahead params"
            ));
        }

        let slot = slot % self.l1_slots_per_epoch;

        if self.lookahead_params[slot as usize].preconfer
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
            || self.lookahead_params[(slot_mod_slots_per_epoch + 1) as usize].preconfer
                != self.ethereum_l1.execution_layer.get_preconfer_address()
    }

    pub fn should_post_lookahead(&self) -> bool {
        self.should_post_lookahead_for_next_epoch
    }

    pub async fn find_slots_to_preconfirm(
        &mut self,
        epoch_begin_timestamp: u64,
        lookahead: &[ProposerDuty],
    ) -> Result<(), Error> {
        if lookahead.len() != self.l1_slots_per_epoch as usize {
            return Err(anyhow::anyhow!(
                "Operator::find_slots_to_preconfirm: unexpected number of proposer duties in the lookahead"
            ));
        }

        let slots = self.l1_slots_per_epoch as usize;
        let validator_bls_pub_keys: Vec<BLSCompressedPublicKey> = lookahead
            .iter()
            .take(slots)
            .map(|key| {
                let mut array = [0u8; 48];
                array.copy_from_slice(&key.public_key);
                array
            })
            .collect();

        self.lookahead_params = self
            .ethereum_l1
            .execution_layer
            .get_lookahead_params_for_epoch(
                epoch_begin_timestamp,
                validator_bls_pub_keys.as_slice().try_into()?,
            )
            .await?;

        self.should_post_lookahead_for_next_epoch = self
            .ethereum_l1
            .execution_layer
            .is_lookahead_required(epoch_begin_timestamp)
            .await?;

        Ok(())
    }
}
