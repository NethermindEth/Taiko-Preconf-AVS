use crate::{
    ethereum_l1::{
        EthereumL1,
        execution_layer::{ExecutionLayer, PreconfOperator},
        extension::ELExtension,
        slot_clock::{Clock, RealClock, SlotClock},
    },
    shared::l2_slot_info::L2SlotInfo,
    taiko::{PreconfDriver, Taiko, preconf_blocks::TaikoStatus},
    utils::types::*,
};
use anyhow::Error;
use std::marker::PhantomData;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::warn;

pub struct Operator<
    ELE: ELExtension,
    T: PreconfOperator = ExecutionLayer<ELE>,
    U: Clock = RealClock,
    V: PreconfDriver = Taiko<ELE>,
> {
    execution_layer: Arc<T>,
    slot_clock: Arc<SlotClock<U>>,
    taiko: Arc<V>,
    handover_window_slots: u64,
    handover_start_buffer_ms: u64,
    next_operator: bool,
    continuing_role: bool,
    simulate_not_submitting_at_the_end_of_epoch: bool,
    was_synced_preconfer: bool,
    cancel_token: CancellationToken,
    cancel_counter: u64,
    operator_transition_slots: u64,
    _phantom: PhantomData<ELE>, // Add this line
}

#[derive(Debug, PartialEq, Eq)]
pub struct Status {
    preconfer: bool,
    submitter: bool,
    preconfirmation_started: bool,
    end_of_sequencing: bool,
    is_driver_synced: bool,
}

impl Status {
    pub fn is_preconfer(&self) -> bool {
        self.preconfer
    }

    pub fn is_submitter(&self) -> bool {
        self.submitter
    }

    pub fn is_driver_synced(&self) -> bool {
        self.is_driver_synced
    }

    pub fn is_preconfirmation_start_slot(&self) -> bool {
        self.preconfirmation_started
    }

    pub fn is_end_of_sequencing(&self) -> bool {
        self.end_of_sequencing
    }
}

const OPERATOR_TRANSITION_SLOTS: u64 = 2;

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut roles = Vec::new();

        if self.preconfer {
            roles.push("Preconf");
        }

        if self.submitter {
            roles.push("Submit");
        }

        if self.preconfer && self.is_driver_synced {
            roles.push("Synced");
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

impl<ELE: ELExtension> Operator<ELE> {
    pub fn new(
        ethereum_l1: &EthereumL1<ELE>,
        taiko: Arc<Taiko<ELE>>,
        handover_window_slots: u64,
        handover_start_buffer_ms: u64,
        simulate_not_submitting_at_the_end_of_epoch: bool,
        cancel_token: CancellationToken,
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
            was_synced_preconfer: false,
            cancel_token,
            cancel_counter: 0,
            operator_transition_slots: OPERATOR_TRANSITION_SLOTS,
            _phantom: PhantomData,
        })
    }
}

impl<ELE: ELExtension, T: PreconfOperator, U: Clock, V: PreconfDriver> Operator<ELE, T, U, V> {
    /// Get the current status of the operator based on the current L1 and L2 slots
    pub async fn get_status(&mut self, l2_slot_info: &L2SlotInfo) -> Result<Status, Error> {
        if !self
            .execution_layer
            .is_preconf_router_specified_in_taiko_wrapper()
            .await?
        {
            warn!("PreconfRouter is not specified in TaikoWrapper");
            self.reset();
            return Ok(Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: false,
            });
        }

        let l1_slot = self.slot_clock.get_current_slot_of_epoch()?;

        // For the first N slots of the new epoch, use the next operator from the previous epoch
        // it's because of the delay that L1 updates the current operator after the epoch has changed.
        let current_operator = if l1_slot < self.operator_transition_slots {
            let curr = match self.execution_layer.is_operator_for_current_epoch().await {
                Ok(val) => format!("{val}"),
                Err(e) => {
                    format!("Failed to check current epoch operator: {e}")
                }
            };
            let next = match self.execution_layer.is_operator_for_next_epoch().await {
                Ok(val) => format!("{val}"),
                Err(e) => {
                    format!("Failed to check next epoch operator: {e}")
                }
            };
            tracing::debug!(
                "Status in transition: l1_slot: {} current_operator: {} next_operator: {}",
                l1_slot,
                curr,
                next
            );
            self.next_operator
        } else {
            self.next_operator = match self.execution_layer.is_operator_for_next_epoch().await {
                Ok(val) => val,
                Err(e) => {
                    warn!("Failed to check next epoch operator: {:?}", e);
                    false
                }
            };
            let current_operator = self.execution_layer.is_operator_for_current_epoch().await?;
            self.continuing_role = current_operator && self.next_operator;
            current_operator
        };

        let handover_window = self.is_handover_window(l1_slot);
        let driver_status = self.taiko.get_status().await?;
        let is_driver_synced = self.is_driver_synced(l2_slot_info, &driver_status).await?;
        let preconfer = self
            .is_preconfer(
                current_operator,
                handover_window,
                l1_slot,
                l2_slot_info,
                &driver_status,
            )
            .await?;
        let preconfirmation_started =
            self.is_preconfirmation_start_l2_slot(preconfer, is_driver_synced);
        if preconfirmation_started {
            self.was_synced_preconfer = true;
        }
        if !preconfer {
            self.was_synced_preconfer = false;
        }

        let submitter = self.is_submitter(current_operator, handover_window);
        let end_of_sequencing = self.is_end_of_sequencing(preconfer, submitter, l1_slot)?;

        Ok(Status {
            preconfer,
            submitter,
            preconfirmation_started,
            end_of_sequencing,
            is_driver_synced,
        })
    }

    pub fn reset(&mut self) {
        self.next_operator = false;
        self.continuing_role = false;
        self.was_synced_preconfer = false;
        self.cancel_counter = 0;
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

    async fn is_driver_synced(
        &mut self,
        l2_slot_info: &L2SlotInfo,
        driver_status: &TaikoStatus,
    ) -> Result<bool, Error> {
        if !self
            .is_block_height_synced_between_taiko_geth_and_the_driver(driver_status, l2_slot_info)
            .await?
        {
            self.cancel_counter += 1;
            self.cancel_if_not_synced_for_sufficient_long_time();
            return Ok(false);
        }
        self.cancel_counter = 0;
        Ok(true)
    }

    async fn is_preconfer(
        &mut self,
        current_operator: bool,
        handover_window: bool,
        l1_slot: Slot,
        l2_slot_info: &L2SlotInfo,
        driver_status: &TaikoStatus,
    ) -> Result<bool, Error> {
        if handover_window {
            return Ok(self.next_operator
                && (self.was_synced_preconfer // If we were the operator for the previous slot, the handover buffer doesn't matter.
                    || !self.is_handover_buffer(l1_slot, l2_slot_info, driver_status).await?));
        }

        Ok(current_operator)
    }

    fn cancel_if_not_synced_for_sufficient_long_time(&mut self) {
        if self.cancel_counter > self.slot_clock.get_l2_slots_per_epoch() / 2 {
            warn!(
                "Not synchronized Geth driver count: {}, exiting...",
                self.cancel_counter
            );
            self.cancel_token.cancel();
        }
    }

    async fn is_handover_buffer(
        &self,
        l1_slot: Slot,
        l2_slot_info: &L2SlotInfo,
        driver_status: &TaikoStatus,
    ) -> Result<bool, Error> {
        if self.get_ms_from_handover_window_start(l1_slot)? <= self.handover_start_buffer_ms {
            tracing::debug!(
                "Is handover buffer, end_of_sequencing_block_hash: {}",
                driver_status.end_of_sequencing_block_hash
            );
            return Ok(!self.end_of_sequencing_marker_received(driver_status, l2_slot_info));
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

    fn is_preconfirmation_start_l2_slot(&self, preconfer: bool, is_driver_synced: bool) -> bool {
        !self.was_synced_preconfer && preconfer && is_driver_synced
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

    async fn is_block_height_synced_between_taiko_geth_and_the_driver(
        &self,
        status: &TaikoStatus,
        l2_slot_info: &L2SlotInfo,
    ) -> Result<bool, Error> {
        if status.highest_unsafe_l2_payload_block_id == 0 {
            return Ok(true);
        }

        let taiko_geth_height = l2_slot_info.parent_id();
        if taiko_geth_height != status.highest_unsafe_l2_payload_block_id {
            warn!(
                "highestUnsafeL2PayloadBlockID: {}, different from Taiko Geth Height: {}",
                status.highest_unsafe_l2_payload_block_id, taiko_geth_height
            );
        }

        Ok(taiko_geth_height == status.highest_unsafe_l2_payload_block_id)
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
        is_preconf_router_specified: bool,
    }

    impl PreconfOperator for ExecutionLayerMock {
        async fn is_operator_for_current_epoch(&self) -> Result<bool, Error> {
            Ok(self.current_operator)
        }

        async fn is_operator_for_next_epoch(&self) -> Result<bool, Error> {
            Ok(self.next_operator)
        }

        async fn is_preconf_router_specified_in_taiko_wrapper(&self) -> Result<bool, Error> {
            Ok(self.is_preconf_router_specified)
        }
    }

    struct TaikoUnsyncedMock {
        end_of_sequencing_block_hash: B256,
    }

    impl PreconfDriver for TaikoUnsyncedMock {
        async fn get_status(&self) -> Result<preconf_blocks::TaikoStatus, Error> {
            Ok(preconf_blocks::TaikoStatus {
                end_of_sequencing_block_hash: self.end_of_sequencing_block_hash,
                highest_unsafe_l2_payload_block_id: 2,
            })
        }
    }

    struct TaikoMock {
        end_of_sequencing_block_hash: B256,
    }
    impl PreconfDriver for TaikoMock {
        async fn get_status(&self) -> Result<preconf_blocks::TaikoStatus, Error> {
            Ok(preconf_blocks::TaikoStatus {
                end_of_sequencing_block_hash: self.end_of_sequencing_block_hash,
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
    async fn test_preconf_router_not_specified() {
        let mut operator = create_operator(
            32 * 12 + 2, // first l1 slot, second l2 slot
            true,
            false,
            false,
        );
        operator.next_operator = true;
        operator.was_synced_preconfer = true;
        operator.continuing_role = false;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: false,
            },
        );
    }

    #[tokio::test]
    async fn test_end_of_sequencing() {
        // End of sequencing
        let mut operator = create_operator(
            (31 - HANDOVER_WINDOW_SLOTS) * 12 + 5 * 2, // l1 slot before handover window, 5th l2 slot
            true,
            false,
            true,
        );
        operator.next_operator = false;
        operator.was_synced_preconfer = true;
        operator.continuing_role = false;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: true,
                is_driver_synced: true,
            }
        );
        // Not a preconfer and submiter
        let mut operator = create_operator(
            (31 - HANDOVER_WINDOW_SLOTS) * 12 + 5 * 2, // l1 slot before handover window, 5th l2 slot
            false,
            false,
            true,
        );
        operator.next_operator = false;
        operator.was_synced_preconfer = false;
        operator.continuing_role = false;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );
        // Continuing role
        let mut operator = create_operator(
            (31 - HANDOVER_WINDOW_SLOTS) * 12 + 5 * 2, // l1 slot before handover window, 5th l2 slot
            true,
            true,
            true,
        );
        operator.next_operator = true;
        operator.was_synced_preconfer = true;
        operator.continuing_role = true;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );
        // Not correct l2 slot
        let mut operator = create_operator(
            (31 - HANDOVER_WINDOW_SLOTS) * 12 + 4 * 2, // l1 slot before handover window, 4th l2 slot
            true,
            false,
            true,
        );
        operator.next_operator = false;
        operator.was_synced_preconfer = true;
        operator.continuing_role = false;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );
    }

    #[tokio::test]
    async fn test_get_preconfer_and_verifier_status() {
        let mut operator = create_operator(
            32 * 12 + 2, // first l1 slot, second l2 slot
            true,
            false,
            true,
        );
        operator.next_operator = true;
        operator.was_synced_preconfer = true;
        operator.continuing_role = false;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );

        let mut operator = create_operator(
            32 * 12 + 2, // first l1 slot, second l2 slot
            false,
            false,
            true,
        );
        operator.was_synced_preconfer = true;
        operator.continuing_role = true;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );
    }

    #[tokio::test]
    async fn test_get_second_slot_status() {
        let mut operator = create_operator(
            32 * 12 + 12 + 2, // second l1 slot, second l2 slot
            true,
            false,
            true,
        );
        operator.next_operator = true;
        operator.was_synced_preconfer = true;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );

        let mut operator = create_operator(
            32 * 12 + 12 + 2, // second l1 slot, second l2 slot
            false,
            false,
            true,
        );
        operator.was_synced_preconfer = true;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );
    }

    #[tokio::test]
    async fn test_is_driver_synced_status() {
        let mut operator = create_operator_with_unsynced_driver_and_geth(
            31 * 12, // last slot of epoch
            false,
            true,
            true,
        );
        operator.was_synced_preconfer = true;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: false,
            }
        );
    }

    #[tokio::test]
    async fn test_get_preconfer_status() {
        let mut operator = create_operator(
            31 * 12, // last slot of epoch
            false,
            true,
            true,
        );
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                preconfirmation_started: true,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );

        let mut operator = create_operator(
            32 * 12, // first slot of next epoch
            true,
            false,
            true,
        );
        operator.next_operator = true;
        operator.was_synced_preconfer = true;
        operator.continuing_role = false;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );

        let mut operator = create_operator(
            32 * 12, // first slot of next epoch
            true,
            false,
            true,
        );
        operator.next_operator = true;
        operator.was_synced_preconfer = true;
        operator.continuing_role = true;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
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
            true,
        );
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );

        // First slot of epoch, not nominated
        let mut operator = create_operator(
            32 * 12, // first slot of next epoch
            false,
            false,
            true,
        );
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );

        let mut operator = create_operator(
            31 * 12, // last slot
            false,
            false,
            true,
        );
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: false,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
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
                is_driver_synced: true,
            }
        );

        let mut operator = create_operator(
            (32 - HANDOVER_WINDOW_SLOTS + 1) * 12, // handover window after the buffer
            false,
            true,
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
                is_driver_synced: true,
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
                is_driver_synced: true,
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
            true,
        );
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: true,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );

        // Current operator outside handover window
        let mut operator = create_operator(
            20 * 12, // middle of epoch
            true,
            false,
            true,
        );
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: true,
                end_of_sequencing: false,
                is_driver_synced: true,
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
            true,
        );
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: false,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );
    }

    #[tokio::test]
    async fn test_get_l1_statuses_for_operator_continuing_role() {
        let mut operator = create_operator(
            0, // first slot of epoch
            true, true, true,
        );
        operator.next_operator = true;
        operator.continuing_role = true;
        operator.was_synced_preconfer = true;

        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );

        let mut operator = create_operator(
            12, // second slot of epoch
            true, true, true,
        );
        operator.next_operator = true;
        operator.continuing_role = true;
        operator.was_synced_preconfer = true;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );

        let mut operator = create_operator(
            2 * 12, // third slot of epoch
            true,
            true,
            true,
        );
        operator.continuing_role = true;
        operator.was_synced_preconfer = true;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: true,
                preconfirmation_started: false,
                end_of_sequencing: false,
                is_driver_synced: true,
            }
        );
    }

    #[tokio::test]
    async fn test_get_preconfirmation_started_status() {
        let mut operator = create_operator(
            31 * 12, // last slot of epoch
            false,
            true,
            true,
        );
        operator.was_synced_preconfer = false;
        assert_eq!(
            operator.get_status(&get_l2_slot_info()).await.unwrap(),
            Status {
                preconfer: true,
                submitter: false,
                preconfirmation_started: true,
                end_of_sequencing: false,
                is_driver_synced: true,
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
                is_driver_synced: true,
            }
        );
    }

    fn create_operator(
        timestamp: i64,
        current_operator: bool,
        next_operator: bool,
        is_preconf_router_specified: bool,
    ) -> Operator<ExecutionLayerMock, MockClock, TaikoMock> {
        let mut slot_clock = SlotClock::<MockClock>::new(0, 0, 12, 32, 2000);
        slot_clock.clock.timestamp = timestamp; // second l1 slot, second l2 slot
        Operator {
            cancel_token: CancellationToken::new(),
            cancel_counter: 0,
            taiko: Arc::new(TaikoMock {
                end_of_sequencing_block_hash: B256::ZERO,
            }),
            execution_layer: Arc::new(ExecutionLayerMock {
                current_operator,
                next_operator,
                is_preconf_router_specified,
            }),
            slot_clock: Arc::new(slot_clock),
            handover_window_slots: HANDOVER_WINDOW_SLOTS as u64,
            handover_start_buffer_ms: 1000,
            next_operator: false,
            continuing_role: false,
            simulate_not_submitting_at_the_end_of_epoch: false,
            was_synced_preconfer: false,
            operator_transition_slots: 1,
            _phantom: PhantomData,
        }
    }

    fn create_operator_with_end_of_sequencing_marker_received(
        timestamp: i64,
        current_operator: bool,
        next_operator: bool,
        is_preconf_router_specified: bool,
    ) -> Operator<ExecutionLayerMock, MockClock, TaikoMock> {
        let mut slot_clock = SlotClock::<MockClock>::new(0, 0, 12, 32, 2000);
        slot_clock.clock.timestamp = timestamp; // second l1 slot, second l2 slot
        Operator {
            cancel_token: CancellationToken::new(),
            taiko: Arc::new(TaikoMock {
                end_of_sequencing_block_hash: get_test_hash(),
            }),
            execution_layer: Arc::new(ExecutionLayerMock {
                current_operator,
                next_operator,
                is_preconf_router_specified,
            }),
            slot_clock: Arc::new(slot_clock),
            handover_window_slots: HANDOVER_WINDOW_SLOTS as u64,
            handover_start_buffer_ms: 1000,
            next_operator: false,
            continuing_role: false,
            simulate_not_submitting_at_the_end_of_epoch: false,
            was_synced_preconfer: false,
            cancel_counter: 0,
            operator_transition_slots: 1,
            _phantom: PhantomData,
        }
    }

    fn create_operator_with_unsynced_driver_and_geth(
        timestamp: i64,
        current_operator: bool,
        next_operator: bool,
        is_preconf_router_specified: bool,
    ) -> Operator<ExecutionLayerMock, MockClock, TaikoUnsyncedMock> {
        let mut slot_clock = SlotClock::<MockClock>::new(0, 0, 12, 32, 2000);
        slot_clock.clock.timestamp = timestamp; // second l1 slot, second l2 slot
        Operator {
            cancel_token: CancellationToken::new(),
            taiko: Arc::new(TaikoUnsyncedMock {
                end_of_sequencing_block_hash: get_test_hash(),
            }),
            execution_layer: Arc::new(ExecutionLayerMock {
                current_operator,
                next_operator,
                is_preconf_router_specified,
            }),
            slot_clock: Arc::new(slot_clock),
            handover_window_slots: HANDOVER_WINDOW_SLOTS as u64,
            handover_start_buffer_ms: 1000,
            next_operator: false,
            continuing_role: false,
            simulate_not_submitting_at_the_end_of_epoch: false,
            was_synced_preconfer: false,
            cancel_counter: 0,
            operator_transition_slots: 1,
            _phantom: PhantomData,
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
