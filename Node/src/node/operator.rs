use crate::ethereum_l1::{slot_clock::Slot, EthereumL1};
use anyhow::Error;
use beacon_api_client::ProposerDuty;
use std::sync::Arc;

pub struct Operator {
    ethereum_l1: Arc<EthereumL1>,
    first_slot_to_preconf: Option<Slot>,
    final_slot_to_preconf: Option<Slot>,
    should_post_lookahead_for_next_epoch: bool,
}

pub enum Status {
    None,
    Preconfer,
    PreconferAndProposer, // has to force include transactions
}

impl Operator {
    pub fn new(ethereum_l1: Arc<EthereumL1>) -> Self {
        Self {
            ethereum_l1,
            first_slot_to_preconf: None,
            final_slot_to_preconf: None,
            should_post_lookahead_for_next_epoch: false,
        }
    }

    pub fn get_status(&self, slot: Slot) -> Status {
        if let (Some(first_slot), Some(final_slot)) =
            (self.first_slot_to_preconf, self.final_slot_to_preconf)
        {
            if slot == final_slot {
                return Status::PreconferAndProposer;
            }
            if slot >= first_slot && slot < final_slot {
                return Status::Preconfer;
            }
        }
        Status::None
    }

    pub fn should_post_lookahead(&self) -> bool {
        self.should_post_lookahead_for_next_epoch
    }

    pub async fn find_slots_to_preconfirm(
        &mut self,
        lookahead: &[ProposerDuty],
    ) -> Result<(), Error> {
        let first_duty = if let Some(duty) = lookahead.first() {
            duty
        } else {
            tracing::error!("Empty lookahead");
            return Ok(());
        };
        let mut first_slot_to_preconf = first_duty.slot;
        let mut first_preconfer = true;
        let mut preconfer_found = false;
        let avs_node_address = self.ethereum_l1.execution_layer.get_preconfer_address();
        self.should_post_lookahead_for_next_epoch = false;

        for duty in lookahead {
            if let Some(preconfer) = self
                .get_preconfer_for_the_slot(
                    duty,
                    self.ethereum_l1.slot_clock.start_of(duty.slot)?.as_secs(),
                )
                .await?
            {
                if preconfer == avs_node_address {
                    self.first_slot_to_preconf = Some(first_slot_to_preconf);
                    self.final_slot_to_preconf = Some(duty.slot);
                    if first_preconfer {
                        self.should_post_lookahead_for_next_epoch = true;
                    }
                    return Ok(());
                }
                first_preconfer = false;
                first_slot_to_preconf = duty.slot + 1;
                preconfer_found = true;
            }
        }

        // no preconfers in the current epoch
        if !preconfer_found {
            // TODO: ask the contract for the randomly chosen preconfer for whole epoch
        }

        Ok(())
    }

    async fn get_preconfer_for_the_slot(
        &self,
        duty: &ProposerDuty,
        slot_begin_timestamp: u64,
    ) -> Result<Option<[u8; 20]>, Error> {
        let validator = self
            .ethereum_l1
            .execution_layer
            .get_validator(&duty.public_key.to_vec())
            .await?;

        if validator.preconfer == [0u8; 20] {
            return Ok(None);
        }

        if slot_begin_timestamp < validator.start_proposing_at
            || (validator.stop_proposing_at != 0
                && slot_begin_timestamp > validator.stop_proposing_at)
        {
            return Ok(None);
        }

        Ok(Some(validator.preconfer))
    }
}
