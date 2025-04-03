use crate::{ethereum_l1::EthereumL1, utils::types::*};
use anyhow::Error;
use std::sync::Arc;
use tracing::debug;
pub struct Operator {
    ethereum_l1: Arc<EthereumL1>,
    handover_window_slots: u64,
    handover_start_buffer_ms: u64,
    nominated_for_next_operator: bool,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Status {
    None,                         // not an operator
    Preconfer,                    // handover window before being an operator, can preconfirm only
    PreconferHandoverBuffer(u64), // beginning of handover window, need to wait given milliseconds before preconfirming
    PreconferAndL1Submitter, // preconfing and submitting period before handover window for next preconfer
    L1Submitter,             // handover window for next operator, can submit only
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::None => write!(f, "Not my slot to preconfirm"),
            Status::Preconfer => write!(f, "Preconfirming"),
            Status::PreconferHandoverBuffer(_) => write!(f, "Preconfirming after handover buffer"),
            Status::PreconferAndL1Submitter => write!(f, "Preconfirming and submitting"),
            Status::L1Submitter => write!(f, "Submitting left batches"),
        }
    }
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
            nominated_for_next_operator: false,
        })
    }

    pub async fn get_status(&mut self) -> Result<(Status, String), Error> {
        let l1_slot = self.ethereum_l1.slot_clock.get_current_slot_of_epoch()?;
        let l2_slot = self
            .ethereum_l1
            .slot_clock
            .get_current_l2_slot_within_l1_slot()?;

        // For the first L1 slot and the first L2 slot of second L1 slot,
        // use the next operator from the previous epoch
        // it's because of the delay that L1 updates the current operator
        // after the epoch has changed.
        if l1_slot == 0 || (l1_slot == 1 && l2_slot == 0) {
            if self.nominated_for_next_operator {
                return Ok((
                    Status::PreconferAndL1Submitter,
                    "0/1 slot From nomiated".to_string(),
                ));
            } else {
                return Ok((Status::None, "0/1 slot".to_string()));
            }
        }

        let is_current_operator = self
            .ethereum_l1
            .execution_layer
            .is_operator_for_current_epoch()
            .await?;

        if self.is_handover_window(l1_slot)? {
            let next_operator = self
                .ethereum_l1
                .execution_layer
                .is_operator_for_next_epoch()
                .await?;
            if next_operator != self.nominated_for_next_operator {
                debug!(
                    "Changing next operator from {} to {}",
                    self.nominated_for_next_operator, next_operator
                );
            }
            self.nominated_for_next_operator = next_operator;
            if is_current_operator {
                if next_operator {
                    return Ok((
                        Status::PreconferAndL1Submitter,
                        "HW: current and next operator".to_string(),
                    ));
                }
                return Ok((Status::L1Submitter, "HW: current operator".to_string()));
            }
            if next_operator {
                let time_elapsed_since_handover_start = self.get_ms_from_handover_window_start()?;
                if self.handover_start_buffer_ms > time_elapsed_since_handover_start {
                    return Ok((
                        Status::PreconferHandoverBuffer(
                            self.handover_start_buffer_ms - time_elapsed_since_handover_start,
                        ),
                        "HW: next operator, buffer".to_string(),
                    ));
                }
                return Ok((
                    Status::Preconfer,
                    "HW: next operator, preconfirm".to_string(),
                ));
            }
            return Ok((Status::None, "HW: not an operator".to_string()));
        }

        if is_current_operator {
            return Ok((
                Status::PreconferAndL1Submitter,
                "current operator".to_string(),
            ));
        }

        Ok((Status::None, "end of function".to_string()))
    }

    fn is_handover_window(&self, slot: Slot) -> Result<bool, Error> {
        self.ethereum_l1
            .slot_clock
            .is_slot_in_last_n_slots_of_epoch(slot, self.handover_window_slots)
    }

    fn get_ms_from_handover_window_start(&self) -> Result<u64, Error> {
        let result: u64 = self
            .ethereum_l1
            .slot_clock
            .time_from_n_last_slots_of_epoch(self.handover_window_slots)
            .unwrap()
            .as_millis()
            .try_into()
            .map_err(|err| {
                anyhow::anyhow!("is_handover_window: Field to covert u128 to u64: {:?}", err)
            })?;
        Ok(result)
    }
}
