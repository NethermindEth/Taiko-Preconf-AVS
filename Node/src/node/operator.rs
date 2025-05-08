use crate::{
    ethereum_l1::{
        execution_layer::{ExecutionLayer, PreconfOperator},
        slot_clock::{Clock, RealClock, SlotClock},
        EthereumL1,
    },
    shared::l2_slot_info::L2SlotInfo,
    taiko::{preconf_blocks::TaikoStatus, PreconfDriver, Taiko},
    utils::types::*,
};
use anyhow::Error;
use std::sync::Arc;
use tracing::warn;

pub struct Operator<
    T: PreconfOperator = ExecutionLayer,
    U: Clock = RealClock,
    V: PreconfDriver = Taiko,
> {
    execution_layer: Arc<T>,
    slot_clock: Arc<SlotClock<U>>,
    taiko: Arc<V>,
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
        taiko: Arc<Taiko>,
        handover_window_slots: u64,
        handover_start_buffer_ms: u64,
        simulate_not_submitting_at_the_end_of_epoch: bool,
    ) -> Result<Self, Error> {
        Ok(Self {
            execution_layer: ethereum_l1.execution_layer.clone(),
            slot_clock: ethereum_l1.slot_clock.clone(),
            taiko,
            handover_window_slots,
            handover_start_buffer_ms,
            next_operator: false,
            continuing_role: false,
            simulate_not_submitting_at_the_end_of_epoch,
            was_preconfer: false,
        })
    }
}

impl<T: PreconfOperator, U: Clock, V: PreconfDriver> Operator<T, U, V> {
    /// Get the current status of the operator based on the current L1 and L2 slots
    pub async fn get_status(&mut self, l2_slot_info: &L2SlotInfo) -> Result<Status, Error> {
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
        let preconfer = self
            .is_preconfer(current_operator, handover_window, l1_slot, l2_slot_info)
            .await?;
        let preconfirmation_started = self.is_preconfirmation_start_l2_slot(preconfer);
        self.was_preconfer = preconfer;
        let submitter = self.is_submitter(current_operator, handover_window);
        let end_of_sequencing = self.is_end_of_sequencing(preconfer, submitter, l1_slot)?;

        Ok(Status {
            preconfer,
            submitter,
            preconfirmation_started,
            end_of_sequencing,
        })
    }

    fn is_end_of_sequencing(
        &self,
        preconfer: bool,
        submitter: bool,
        l1_slot: Slot,
    ) -> Result<bool, Error> {
        let slot_before_handover_window = self.is_l2_slot_before_handover_window(l1_slot)?;
        Ok(!self.continuing_role && preconfer && submitter && slot_before_handover_window)
    }

    fn is_l2_slot_before_handover_window(&self, l1_slot: Slot) -> Result<bool, Error> {
        let end_l1_slot = self.slot_clock.get_slots_per_epoch() - self.handover_window_slots - 1;
        if l1_slot == end_l1_slot {
            let l2_slot = self.slot_clock.get_current_l2_slot_within_l1_slot()?;
            Ok(l2_slot + 1 == self.slot_clock.get_number_of_l2_slots_per_l1())
        } else {
            Ok(false)
        }
    }

    async fn is_preconfer(
        &self,
        current_operator: bool,
        handover_window: bool,
        l1_slot: Slot,
        l2_slot_info: &L2SlotInfo,
    ) -> Result<bool, Error> {
        if handover_window {
            return Ok(self.next_operator
                && (current_operator // an operator for current and next epoch, handover buffer doesn't matter
                || !self.is_handover_buffer(l1_slot, l2_slot_info).await?));
        }

        Ok(current_operator)
    }

    async fn is_handover_buffer(
        &self,
        l1_slot: Slot,
        l2_slot_info: &L2SlotInfo,
    ) -> Result<bool, Error> {
        if self.get_ms_from_handover_window_start(l1_slot)? <= self.handover_start_buffer_ms {
            let driver_status = self.taiko.get_status().await?;
            tracing::debug!(
                "Is handover buffer, end_of_sequencing_block_hash: {}",
                driver_status.end_of_sequencing_block_hash
            );
            return Ok(!self.end_of_sequencing_marker_received(&driver_status, l2_slot_info));
        }

        Ok(false)
    }

    fn end_of_sequencing_marker_received(
        &self,
        driver_status: &TaikoStatus,
        l2_slot_info: &L2SlotInfo,
    ) -> bool {
        *l2_slot_info.parent_hash() == driver_status.end_of_sequencing_block_hash
    }

    fn is_submitter(&self, current_operator: bool, handover_window: bool) -> bool {
        if handover_window && self.simulate_not_submitting_at_the_end_of_epoch {
            return false;
        }

        current_operator
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
    use crate::taiko::preconf_blocks;
    const HANDOVER_WINDOW_SLOTS: i64 = 6;
    use alloy::primitives::B256;
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

    struct TaikoMock {
        end_of_sequencing_block_hash: B256,
    }

    impl PreconfDriver for TaikoMock {
        async fn get_status(&self) -> Result<preconf_blocks::TaikoStatus, Error> {
            Ok(preconf_blocks::TaikoStatus {
                end_of_sequencing_block_hash: self.end_of_sequencing_block_hash.clone(),
                highest_unsafe_l2_payload_block_id: 0,
            })
        }
    }

    fn get_l2_slot_info() -> L2SlotInfo {
        L2SlotInfo::new(
            0,
            0,
            0,
            B256::from([
                0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1,
                0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1, 0x1,
            ]),
            0,
        )
    }

    #[tokio::test]
    async fn test_end_of_sequencing() {
        // End of sequencing
        let mut operator = create_operator(
            (31 - HANDOVER_WINDOW_SLOTS) * 12 + 5 * 2, // l1 slot before handover window, 5th l2 slot
            true,
            false,
        );
        operator.next_operator = false;
        operator.was_preconfer = true;
        operator.continuing_role = false;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: true,
            }
        );
        // Not a preconfer and submiter
        let mut operator = create_operator(
            (31 - HANDOVER_WINDOW_SLOTS) * 12 + 5 * 2, // l1 slot before handover window, 5th l2 slot
            false,
            false,
        );
        operator.next_operator = false;
        operator.was_preconfer = false;
        operator.continuing_role = false;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
            }
        );
        // Continuing role
        let mut operator = create_operator(
            (31 - HANDOVER_WINDOW_SLOTS) * 12 + 5 * 2, // l1 slot before handover window, 5th l2 slot
            true,
            true,
        );
        operator.next_operator = true;
        operator.was_preconfer = true;
        operator.continuing_role = true;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
            }
        );
        // Not correct l2 slot
        let mut operator = create_operator(
            (31 - HANDOVER_WINDOW_SLOTS) * 12 + 4 * 2, // l1 slot before handover window, 4th l2 slot
            true,
            false,
        );
        operator.next_operator = false;
        operator.was_preconfer = true;
        operator.continuing_role = false;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
            }
        );
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
            }
        );

        let mut operator = create_operator(
            32 * 12 + 12 + 2, // second l1 slot, second l2 slot
            false,
            false,
        );
        operator.was_preconfer = true;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                preconfirmation_started: true,
                end_of_sequencing: false,
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
            }
        );

        // First slot of epoch, not nominated
        let mut operator = create_operator(
            32 * 12, // first slot of next epoch
            false,
            false,
        );
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
            }
        );

        let mut operator = create_operator(
            31 * 12, // last slot
            false,
            false,
        );
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
            }
        );
    }

    #[tokio::test]
    async fn test_get_preconfer_handover_buffer_status() {
        // Next operator in handover window, but still in buffer period
        let mut operator = create_operator(
            (32 - HANDOVER_WINDOW_SLOTS) * 12, // handover buffer
            false,
            true,
        );
        // Override the handover start buffer to be larger than the mock timestamp
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
            }
        );

        let mut operator = create_operator(
            (32 - HANDOVER_WINDOW_SLOTS + 1) * 12, // handover window after the buffer
            false,
            true,
        );
        // Override the handover start buffer to be larger than the mock timestamp
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                preconfirmation_started: true,
                end_of_sequencing: false,
            }
        );
    }

    #[tokio::test]
    async fn test_get_preconfer_handover_buffer_status_with_end_of_sequencing_marker_received() {
        // Next operator in handover window, but still in buffer period
        let mut operator = create_operator_with_end_of_sequencing_marker_received(
            (32 - HANDOVER_WINDOW_SLOTS) * 12, // handover buffer
            false,
            true,
        );
        // Override the handover start buffer to be larger than the mock timestamp
        assert_eq!(
            operator
                .get_status(&L2SlotInfo::new(0, 0, 0, get_test_hash(), 0))
                .await
                .unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                preconfirmation_started: true,
                end_of_sequencing: false,
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: true,
                end_of_sequencing: false,
            }
        );

        // Current operator outside handover window
        let mut operator = create_operator(
            20 * 12, // middle of epoch
            true,
            false,
        );
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: true,
                end_of_sequencing: false,
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
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
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                preconfirmation_started: true,
                end_of_sequencing: false,
            }
        );

        // second get_status call, preconfirmation_started should be false
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
            }
        );
    }

    fn create_operator(
        timestamp: i64,
        current_operator: bool,
        next_operator: bool,
    ) -> Operator<ExecutionLayerMock, MockClock, TaikoMock> {
        let mut slot_clock = SlotClock::<MockClock>::new(0, 0, 12, 32, 2000);
        slot_clock.clock.timestamp = timestamp; // second l1 slot, second l2 slot
        Operator {
            taiko: Arc::new(TaikoMock {
                end_of_sequencing_block_hash: B256::ZERO,
            }),
            execution_layer: Arc::new(ExecutionLayerMock {
                current_operator,
                next_operator,
            }),
            slot_clock: Arc::new(slot_clock),
            handover_window_slots: HANDOVER_WINDOW_SLOTS as u64,
            handover_start_buffer_ms: 1000,
            next_operator: false,
            continuing_role: false,
            simulate_not_submitting_at_the_end_of_epoch: false,
            was_preconfer: false,
        }
    }

    fn create_operator_with_end_of_sequencing_marker_received(
        timestamp: i64,
        current_operator: bool,
        next_operator: bool,
    ) -> Operator<ExecutionLayerMock, MockClock, TaikoMock> {
        let mut slot_clock = SlotClock::<MockClock>::new(0, 0, 12, 32, 2000);
        slot_clock.clock.timestamp = timestamp; // second l1 slot, second l2 slot
        Operator {
            taiko: Arc::new(TaikoMock {
                end_of_sequencing_block_hash: get_test_hash(),
            }),
            execution_layer: Arc::new(ExecutionLayerMock {
                current_operator,
                next_operator,
            }),
            slot_clock: Arc::new(slot_clock),
            handover_window_slots: HANDOVER_WINDOW_SLOTS as u64,
            handover_start_buffer_ms: 1000,
            next_operator: false,
            continuing_role: false,
            simulate_not_submitting_at_the_end_of_epoch: false,
            was_preconfer: false,
        }
    }

    fn get_test_hash() -> B256 {
        B256::from([
            0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab,
            0xcd, 0xef, 0x12, 0x34, 0x56, 0x78, 0x90, 0xab, 0xcd, 0xef, 0x12, 0x34, 0x56, 0x78,
            0x90, 0xab, 0xcd, 0xef,
        ])
    }
}
