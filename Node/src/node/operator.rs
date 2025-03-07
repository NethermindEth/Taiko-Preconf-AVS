use crate::ethereum_l1::EthereumL1;
use anyhow::Error;
use std::sync::Arc;

pub struct Operator {
    ethereum_l1: Arc<EthereumL1>,
    handover_window_slots: u64,
    handover_start_buffer_ms: u64,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Status {
    None,                    // not an operator
    Preconfer,               // handover window before being an operator, can preconfirm only
    PreconferAndL1Submitter, // preconfing and submitting period before handover window for next preconfer
    L1Submitter,             // handover window for next operator, can submit only
}

impl Operator {
    pub fn new(
        ethereum_l1: Arc<EthereumL1>,
        handover_window_slots: u64,
        handover_start_buffer_ms: u64,
    ) -> Result<Self, Error> {
        Ok(Self {
            ethereum_l1,
            handover_window_slots,
            handover_start_buffer_ms,
        })
    }

    pub async fn get_status(&mut self) -> Result<Status, Error> {
        let is_current_operator = self
            .ethereum_l1
            .execution_layer
            .is_operator_for_current_epoch()
            .await?;

        if self.is_handover_window()? {
            if is_current_operator {
                return Ok(Status::L1Submitter);
            }
            if self
                .ethereum_l1
                .execution_layer
                .is_operator_for_next_epoch()
                .await?
            {
                return Ok(Status::Preconfer);
            }
            return Ok(Status::None);
        }

        if is_current_operator {
            return Ok(Status::PreconferAndL1Submitter);
        }

        Ok(Status::None)
    }

    pub fn is_handover_window(&self) -> Result<bool, Error> {
        let slot = self.ethereum_l1.slot_clock.get_current_slot_of_epoch()?;

        if self
            .ethereum_l1
            .slot_clock
            .is_slot_in_last_n_slots_of_epoch(slot, self.handover_window_slots)?
        {
            let time_millis: u64 = self
                .ethereum_l1
                .slot_clock
                .time_from_n_last_slots_of_epoch(self.handover_window_slots)
                .unwrap()
                .as_millis()
                .try_into()
                .map_err(|err| {
                    anyhow::anyhow!("is_handover_window: Field to covert u128 to u64: {:?}", err)
                })?;
            return Ok(time_millis > self.handover_start_buffer_ms);
        }

        Ok(false)
    }
}
