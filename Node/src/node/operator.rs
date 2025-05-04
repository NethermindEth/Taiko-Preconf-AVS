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
use tracing::warn;

pub struct Operator<T: PreconfOperator = ExecutionLayer, U: Clock = RealClock> {
    execution_layer: Arc<T>,
    slot_clock: Arc<SlotClock<U>>,
    handover_window_slots: u64,
    handover_start_buffer_ms: u64,
    next_operator: bool,
    continuing_role: bool,
    simulate_not_submitting_at_the_end_of_epoch: bool,
    was_preconfer: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Status {
    preconfer: bool,
    submitter: bool,
    verifier: bool,
    preconfirmation_started: bool,
    end_of_sequencing: bool,
}

impl Status {
    pub fn is_preconfer(&self) -> bool {
        self.preconfer
    }

    pub fn is_submitter(&self) -> bool {
        self.submitter
    }

    pub fn is_verifier(&self) -> bool {
        self.verifier
    }

    pub fn is_preconfirmation_start_slot(&self) -> bool {
        self.preconfirmation_started
    }

    pub fn is_end_of_sequencing(&self) -> bool {
        self.end_of_sequencing
    }
}

const OPERATOR_TRANSITION_SLOTS: u64 = 1;

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut roles = Vec::new();

        if self.preconfer {
            roles.push("Preconf");
        }

        if self.submitter {
            roles.push("Submit");
        }

        if self.verifier {
            roles.push("Verify");
        }

        if self.end_of_sequencing {
            roles.push("EndOfSequencing");
        }

        if roles.is_empty() {
            write!(f, "No active roles")
        } else {
            write!(f, "{}", roles.join(", "))
        }
    }
}

impl Operator {
    pub fn new(
        ethereum_l1: &EthereumL1,
        handover_window_slots: u64,
        handover_start_buffer_ms: u64,
        simulate_not_submitting_at_the_end_of_epoch: bool,
    ) -> Result<Self, Error> {
        Ok(Self {
            execution_layer: ethereum_l1.execution_layer.clone(),
            slot_clock: ethereum_l1.slot_clock.clone(),
            handover_window_slots,
            handover_start_buffer_ms,
            next_operator: false,
            continuing_role: false,
            simulate_not_submitting_at_the_end_of_epoch,
            was_preconfer: false,
        })
    }
}

impl<T: PreconfOperator, U: Clock> Operator<T, U> {
    /// Get the current status of the operator based on the current L1 and L2 slots
    pub async fn get_status(&mut self) -> Result<Status, Error> {
        let l1_slot = self.slot_clock.get_current_slot_of_epoch()?;

        // For the first N slots of the new epoch, use the next operator from the previous epoch
        // it's because of the delay that L1 updates the current operator after the epoch has changed.
        let current_operator = if l1_slot < OPERATOR_TRANSITION_SLOTS {
            self.next_operator
        } else {
            self.next_operator = match self.execution_layer.is_operator_for_next_epoch().await {
                Ok(val) => val,
                Err(e) => {
                    warn!("Warning: failed to check next epoch operator: {:?}", e);
                    false
                }
            };
            let current_operator = self.execution_layer.is_operator_for_current_epoch().await?;
            self.continuing_role = current_operator && self.next_operator;
            current_operator
        };

        let handover_window = self.is_handover_window(l1_slot);
        let preconfer = self.is_preconfer(current_operator, handover_window, l1_slot)?;
        let preconfirmation_started = self.is_preconfirmation_start_l2_slot(preconfer);
        self.was_preconfer = preconfer;
        let end_of_sequencing = self.is_end_of_sequencing(preconfer, submitter, l1_slot)?;

        Ok(Status {
            preconfer,
            submitter: self.is_submitter(l1_slot, current_operator, handover_window),
            verifier: self.is_verifier(l1_slot, current_operator),
            preconfirmation_started,
            end_of_sequencing,
        })
    }

    fn is_end_of_sequencing(&self, preconfer: bool, submitter:bool, l1_slot: Slot) -> Result<bool, Error> {
        let slot_before_handover_window = self.is_l2_slot_before_handover_window(l1_slot);
        Ok(!self.continuing_role && preconfer && submitter && slot_before_handover_window);
    }

    fn is_l2_slot_before_handover_window(&self, l1_slot: Slot) -> Result<bool, Error> {
        let end_l1_slot = self.slot_clock.get_slots_per_epoch() - self.handover_window_slots - 1;
        if l1_slot == end_l1_slot {
            let l2_slot = self.slot_clock.get_current_l2_slot_within_l1_slot()?;
            Ok(l2_slot + 1 == self.slot_clock.get_l2_slots_capacity())
        } else {
            Ok(false)
        }
    }

    fn is_preconfer(
        &self,
        current_operator: bool,
        handover_window: bool,
        l1_slot: Slot,
    ) -> Result<bool, Error> {
        if handover_window {
            return Ok(self.next_operator
                && (current_operator // an operator for current and next epoch, handover buffer doesn't matter
                || !self.is_handover_buffer(l1_slot)?));
        }

        Ok(current_operator)
    }

    fn is_handover_buffer(&self, l1_slot: Slot) -> Result<bool, Error> {
        Ok(self.get_ms_from_handover_window_start(l1_slot)? <= self.handover_start_buffer_ms)
    }

    fn is_submitter(&self, l1_slot: u64, current_operator: bool, handover_window: bool) -> bool {
        if l1_slot < OPERATOR_TRANSITION_SLOTS && !self.continuing_role {
            return false; // do not summit here, it's for verification
        }

        if handover_window && self.simulate_not_submitting_at_the_end_of_epoch {
            return false;
        }

        current_operator
    }

    fn is_verifier(&self, l1_slot: u64, current_operator: bool) -> bool {
        current_operator && l1_slot < OPERATOR_TRANSITION_SLOTS && !self.continuing_role
    }

    fn is_preconfirmation_start_l2_slot(&self, preconfer: bool) -> bool {
        !self.was_preconfer && preconfer
    }

    fn is_handover_window(&self, slot: Slot) -> bool {
        self.slot_clock
            .is_slot_in_last_n_slots_of_epoch(slot, self.handover_window_slots)
    }

    fn get_ms_from_handover_window_start(&self, l1_slot: Slot) -> Result<u64, Error> {
        let result: u64 = self
            .slot_clock
            .time_from_n_last_slots_of_epoch(l1_slot, self.handover_window_slots)?
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
            32 * 12 + 2, // first l1 slot, second l2 slot
            true,
            false,
        );
        operator.next_operator = true;
        operator.was_preconfer = true;
        operator.continuing_role = false;
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                verifier: true,
                preconfirmation_started: false,
            }
        );

        let mut operator = create_operator(
            32 * 12 + 2, // first l1 slot, second l2 slot
            false,
            false,
        );
        operator.was_preconfer = true;
        operator.continuing_role = true;
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                verifier: false,
                preconfirmation_started: false,
            }
        );
    }

    #[tokio::test]
    async fn test_get_second_slot_status() {
        let mut operator = create_operator(
            32 * 12 + 12 + 2, // second l1 slot, second l2 slot
            true,
            false,
        );
        operator.next_operator = true;
        operator.was_preconfer = true;
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                verifier: false,
                preconfirmation_started: false,
            }
        );

        let mut operator = create_operator(
            32 * 12 + 12 + 2, // second l1 slot, second l2 slot
            false,
            false,
        );
        operator.was_preconfer = true;
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                verifier: false,
                preconfirmation_started: false,
            }
        );
    }

    #[tokio::test]
    async fn test_get_preconfer_status() {
        let mut operator = create_operator(
            31 * 12, // last slot of epoch
            false,
            true,
        );
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                verifier: false,
                preconfirmation_started: true,
            }
        );

        let mut operator = create_operator(
            32 * 12, // first slot of next epoch
            true,
            false,
        );
        operator.next_operator = true;
        operator.was_preconfer = true;
        operator.continuing_role = false;
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                verifier: true,
                preconfirmation_started: false,
            }
        );

        let mut operator = create_operator(
            32 * 12, // first slot of next epoch
            true,
            false,
        );
        operator.next_operator = true;
        operator.was_preconfer = true;
        operator.continuing_role = true;
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                verifier: false,
                preconfirmation_started: false,
            }
        );
    }

    #[tokio::test]
    async fn test_get_none_status() {
        // Not an operator at all
        let mut operator = create_operator(
            20 * 12, // middle of epoch
            false,
            false,
        );
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                verifier: false,
                preconfirmation_started: false,
            }
        );

        // First slot of epoch, not nominated
        let mut operator = create_operator(
            32 * 12, // first slot of next epoch
            false,
            false,
        );
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                verifier: false,
                preconfirmation_started: false,
            }
        );

        let mut operator = create_operator(
            31 * 12, // last slot
            false,
            false,
        );
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                verifier: false,
                preconfirmation_started: false,
            }
        );
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
            operator.get_status().await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                verifier: false,
                preconfirmation_started: false,
            }
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
            operator.get_status().await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                verifier: false,
                preconfirmation_started: true,
            }
        );

        // Current operator outside handover window
        let mut operator = create_operator(
            20 * 12, // middle of epoch
            true,
            false,
        );
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                verifier: false,
                preconfirmation_started: true,
            }
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
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: false,
                submitter: true,
                verifier: false,
                preconfirmation_started: false,
            }
        );
    }

    #[tokio::test]
    async fn test_get_l1_statuses_for_operator_continuing_role() {
        let mut operator = create_operator(
            0, // first slot of epoch
            true, true,
        );
        operator.next_operator = true;
        operator.continuing_role = true;
        operator.was_preconfer = true;

        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                verifier: false,
                preconfirmation_started: false,
            }
        );

        let mut operator = create_operator(
            1 * 12, // second slot of epoch
            true,
            true,
        );
        operator.next_operator = true;
        operator.continuing_role = true;
        operator.was_preconfer = true;
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                verifier: false,
                preconfirmation_started: false,
            }
        );

        let mut operator = create_operator(
            2 * 12, // third slot of epoch
            true,
            true,
        );
        operator.continuing_role = true;
        operator.was_preconfer = true;
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                verifier: false,
                preconfirmation_started: false,
            }
        );
    }

    #[tokio::test]
    async fn test_get_preconfirmation_started_status() {
        let mut operator = create_operator(
            31 * 12, // last slot of epoch
            false,
            true,
        );
        operator.was_preconfer = false;
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                verifier: false,
                preconfirmation_started: true,
            }
        );

        // second get_status call, preconfirmation_started should be false
        assert_eq!(
            operator.get_status().await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                verifier: false,
                preconfirmation_started: false,
            }
        );
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
            next_operator: false,
            continuing_role: false,
            simulate_not_submitting_at_the_end_of_epoch: false,
            was_preconfer: false,
        }
    }
}
