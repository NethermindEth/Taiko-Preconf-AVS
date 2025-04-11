use crate::{
    ethereum_l1::{
        execution_layer::{ExecutionLayer, PreconfOperator},
        slot_clock::{Clock, RealClock, SlotClock},
        EthereumL1,
    },
    utils::types::*,
};
use anyhow::Error;
use std::sync::Arc;
use tracing::debug;

pub struct Operator<T: PreconfOperator = ExecutionLayer, U: Clock = RealClock> {
    execution_layer: Arc<T>,
    slot_clock: Arc<SlotClock<U>>,
    handover_window_slots: u64,
    handover_start_buffer_ms: u64,
    nominated_for_next_operator: bool,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Status {
    None,                    // not an operator
    Preconfer,               // handover window before being an operator, can preconfirm only
    PreconferHandoverBuffer, // beginning of the handover window, no preconfirmation
    PreconferAndL1Submitter, // preconfirming and submitting period before handover window for next preconfer
    PreconferAndVerifier,    // preconfirming and verifying inclusion of previous epoch batches
    L1Submitter,             // handover window for next operator, can submit only
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::None => write!(f, "Not my slot to preconfirm"),
            Status::Preconfer => write!(f, "Preconfirming"),
            Status::PreconferHandoverBuffer => write!(f, "Handover buffer"),
            Status::PreconferAndL1Submitter => write!(f, "Preconfirming and submitting"),
            Status::PreconferAndVerifier => write!(f, "Preconfirming and verifying"),
            Status::L1Submitter => write!(f, "Submitting left batches"),
        }
    }
}

impl Operator {
    pub fn new(
        ethereum_l1: &EthereumL1,
        handover_window_slots: u64,
        handover_start_buffer_ms: u64,
    ) -> Result<Self, Error> {
        Ok(Self {
            execution_layer: ethereum_l1.execution_layer.clone(),
            slot_clock: ethereum_l1.slot_clock.clone(),
            handover_window_slots,
            handover_start_buffer_ms,
            nominated_for_next_operator: false,
        })
    }
}

impl<T: PreconfOperator, U: Clock> Operator<T, U> {
    /// Get the current status of the operator based on the current L1 and L2 slots
    /// TODO: remove second string parameter, temporary for debugging
    pub async fn get_status(&mut self) -> Result<(Status, String), Error> {
        let l1_slot = self.slot_clock.get_current_slot_of_epoch()?;
        let l2_slot = self.slot_clock.get_current_l2_slot_within_l1_slot()?;

        // For the first L1 slot and the first L2 slot of second L1 slot,
        // use the next operator from the previous epoch
        // it's because of the delay that L1 updates the current operator
        // after the epoch has changed.
        if l1_slot == 0 || (l1_slot == 1 && l2_slot == 0) {
            if self.nominated_for_next_operator {
                return Ok((Status::Preconfer, "epoch begin nominated".to_string()));
            } else {
                return Ok((Status::None, "epoch begin not nominated".to_string()));
            }
        }

        let current_operator = self.execution_layer.is_operator_for_current_epoch().await?;
        if l1_slot == 1 && current_operator {
            return Ok((Status::PreconferAndVerifier, "verifying".to_string()));
        }

        if self.is_handover_window(l1_slot) {
            let next_operator = self.execution_layer.is_operator_for_next_epoch().await?;
            if next_operator != self.nominated_for_next_operator {
                debug!(
                    "Changing next operator from {} to {}",
                    self.nominated_for_next_operator, next_operator
                );
            }
            self.nominated_for_next_operator = next_operator;
            if current_operator {
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
                        Status::PreconferHandoverBuffer,
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

        if current_operator {
            return Ok((
                Status::PreconferAndL1Submitter,
                "current operator".to_string(),
            ));
        }

        Ok((Status::None, "end of function".to_string()))
    }

    fn is_handover_window(&self, slot: Slot) -> bool {
        self.slot_clock
            .is_slot_in_last_n_slots_of_epoch(slot, self.handover_window_slots)
    }

    fn get_ms_from_handover_window_start(&self) -> Result<u64, Error> {
        let result: u64 = self
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ethereum_l1::slot_clock::mock::*;

    struct ExecutionLayerMock {
        current_operator: bool,
        next_operator: bool,
    }

    impl PreconfOperator for ExecutionLayerMock {
        async fn is_operator_for_current_epoch(&self) -> Result<bool, Error> {
            Ok(self.current_operator)
        }

        async fn is_operator_for_next_epoch(&self) -> Result<bool, Error> {
            Ok(self.next_operator)
        }
    }

    #[tokio::test]
    async fn test_get_preconfer_and_verifier_status() {
        let mut operator = create_operator(
            32 * 12 + 12 + 2, // second l1 slot, second l2 slot
            true,
            false,
        );

        assert_eq!(
            operator.get_status().await.unwrap().0,
            Status::PreconferAndVerifier
        );
    }

    #[tokio::test]
    async fn test_get_preconfer_status() {
        let mut operator = create_operator(
            31 * 12, // last slot of epoch
            false,
            true,
        );
        assert_eq!(operator.get_status().await.unwrap().0, Status::Preconfer);

        let mut operator = create_operator(
            32 * 12, // first slot of next epoch
            true,
            false,
        );
        operator.nominated_for_next_operator = true;
        assert_eq!(operator.get_status().await.unwrap().0, Status::Preconfer);
    }

    #[tokio::test]
    async fn test_get_none_status() {
        // Not an operator at all
        let mut operator = create_operator(
            20 * 12, // middle of epoch
            false,
            false,
        );
        assert_eq!(operator.get_status().await.unwrap().0, Status::None);

        // First slot of epoch, not nominated
        let mut operator = create_operator(
            32 * 12, // first slot of next epoch
            false,
            false,
        );
        assert_eq!(operator.get_status().await.unwrap().0, Status::None);
    }

    #[tokio::test]
    async fn test_get_preconfer_handover_buffer_status() {
        // Next operator in handover window, but still in buffer period
        let mut operator = create_operator(
            (32 - 6) * 12, // handover buffer
            false,
            true,
        );
        // Override the handover start buffer to be larger than the mock timestamp
        assert_eq!(
            operator.get_status().await.unwrap().0,
            Status::PreconferHandoverBuffer
        );
    }

    #[tokio::test]
    async fn test_get_preconfer_and_l1_submitter_status() {
        // Current operator and next operator (continuing role)
        let mut operator = create_operator(
            31 * 12, // last slot of epoch (handover window)
            true,
            true,
        );
        assert_eq!(
            operator.get_status().await.unwrap().0,
            Status::PreconferAndL1Submitter
        );

        // Current operator outside handover window
        let mut operator = create_operator(
            20 * 12, // middle of epoch
            true,
            false,
        );
        assert_eq!(
            operator.get_status().await.unwrap().0,
            Status::PreconferAndL1Submitter
        );
    }

    #[tokio::test]
    async fn test_get_l1_submitter_status() {
        // Current operator but not next operator during handover window
        let mut operator = create_operator(
            31 * 12, // last slot of epoch
            true,
            false,
        );
        assert_eq!(operator.get_status().await.unwrap().0, Status::L1Submitter);
    }

    fn create_operator(
        timestamp: i64,
        current_operator: bool,
        next_operator: bool,
    ) -> Operator<ExecutionLayerMock, MockClock> {
        let mut slot_clock = SlotClock::<MockClock>::new(0, 0, 12, 32, 2000);
        slot_clock.clock.timestamp = timestamp; // second l1 slot, second l2 slot
        Operator {
            execution_layer: Arc::new(ExecutionLayerMock {
                current_operator,
                next_operator,
            }),
            slot_clock: Arc::new(slot_clock),
            handover_window_slots: 6,
            handover_start_buffer_ms: 1000,
            nominated_for_next_operator: false,
        }
    }
}
