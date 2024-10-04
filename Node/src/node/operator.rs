use crate::{
    ethereum_l1::{execution_layer::PreconfTaskManager, EthereumL1},
    utils::types::*,
};
use anyhow::Error;
use std::sync::Arc;
use tracing::{debug, error};

pub struct Operator {
    ethereum_l1: Arc<EthereumL1>,
    epoch: u64,
    lookahead_preconfer_addresses: Vec<PreconferAddress>,
    lookahead_preconfer_buffer: Vec<PreconfTaskManager::LookaheadBufferEntry>,
    l1_slots_per_epoch: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    None,
    Preconfer,
    PreconferAndProposer, // has to force include transactions
}

impl Operator {
    pub fn new(ethereum_l1: Arc<EthereumL1>, epoch: Epoch) -> Result<Self, Error> {
        debug!("Operator::new: epoch: {}", epoch);
        let l1_slots_per_epoch = ethereum_l1.slot_clock.get_slots_per_epoch();
        Ok(Self {
            ethereum_l1,
            epoch,
            lookahead_preconfer_addresses: vec![],
            lookahead_preconfer_buffer: vec![],
            l1_slots_per_epoch,
        })
    }

    #[cfg(debug_assertions)]
    pub async fn print_preconfer_slots(&self, base_slot: Slot) {
        let preconfer = &self.ethereum_l1.execution_layer.get_preconfer_address();
        let preconfer_slots: Vec<String> = self
            .lookahead_preconfer_addresses
            .iter()
            .enumerate()
            .filter_map(|(i, address)| {
                if address == preconfer {
                    Some((base_slot + i as u64).to_string())
                } else {
                    None
                }
            })
            .collect();

        debug!("Preconfer slots: {}", preconfer_slots.join(", "));
    }

    pub async fn get_status(&mut self, slot: Slot) -> Result<Status, Error> {
        if self.lookahead_preconfer_addresses.len() != self.l1_slots_per_epoch as usize {
            return Err(anyhow::anyhow!(
                "Operator::get_status: Incorrect lookahead params, should be {} but {} given",
                self.l1_slots_per_epoch,
                self.lookahead_preconfer_addresses.len()
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
            let next_preconfer_address = self.get_next_preconfer_address(slot).await?;
            if self.is_the_final_slot_to_preconf(next_preconfer_address) {
                return Ok(Status::PreconferAndProposer);
            }
            return Ok(Status::Preconfer);
        }

        Ok(Status::None)
    }

    async fn get_next_preconfer_address(
        &mut self,
        slot_mod_slots_per_epoch: Slot,
    ) -> Result<PreconferAddress, Error> {
        if slot_mod_slots_per_epoch == self.l1_slots_per_epoch - 1 {
            let lookahead_preconfer_addresses_next_epoch = self
                .ethereum_l1
                .execution_layer
                .get_lookahead_preconfer_addresses_for_epoch(self.epoch + 1)
                .await?;
            Ok(lookahead_preconfer_addresses_next_epoch[0])
        } else {
            Ok(self.lookahead_preconfer_addresses[(slot_mod_slots_per_epoch + 1) as usize])
        }
    }

    fn is_the_final_slot_to_preconf(&self, next_preconfer_address: PreconferAddress) -> bool {
        next_preconfer_address != self.ethereum_l1.execution_layer.get_preconfer_address()
    }

    pub async fn should_post_lookahead_for_next_epoch(&mut self) -> Result<bool, Error> {
        self.ethereum_l1
            .execution_layer
            .is_lookahead_required()
            .await
    }

    pub async fn update_preconfer_lookahead_for_epoch(&mut self) -> Result<(), Error> {
        debug!("Updating preconfer lookahead for epoch: {}", self.epoch);

        self.lookahead_preconfer_addresses = self
            .ethereum_l1
            .execution_layer
            .get_lookahead_preconfer_addresses_for_epoch(self.epoch)
            .await?;

        self.lookahead_preconfer_buffer = self
            .ethereum_l1
            .execution_layer
            .get_lookahead_preconfer_buffer()
            .await?
            .to_vec();
        Ok(())
    }

    pub fn get_lookahead_pointer(&mut self, slot: Slot) -> Result<u64, Error> {
        let slot_begin_timestamp = self
            .ethereum_l1
            .slot_clock
            .get_real_slot_begin_timestamp_for_contract(slot)?;

        let lookahead_pointer = self
            .lookahead_preconfer_buffer
            .iter()
            .position(|entry| {
                entry.preconfer == self.ethereum_l1.execution_layer.get_preconfer_address()
                    && slot_begin_timestamp > entry.prevTimestamp
                    && slot_begin_timestamp <= entry.timestamp
            })
            .ok_or_else(|| {
                let buffer_str = self
                    .lookahead_preconfer_buffer
                    .iter()
                    .map(|entry| {
                        format!(
                            "{}, {}, {}, {}",
                            entry.isFallback, entry.timestamp, entry.prevTimestamp, entry.preconfer
                        )
                    })
                    .collect::<Vec<String>>()
                    .join("; ");
                debug!("slot_begin_timestamp: {}", slot_begin_timestamp);
                debug!("Lookahead buffer: [{}]", buffer_str);
                anyhow::anyhow!("get_lookahead_params: Preconfer not found in lookahead")
            })? as u64;

        Ok(lookahead_pointer)
    }

    pub async fn check_empty_lookahead(&mut self) -> Result<(), Error> {
        debug!("Checking empty lookahead");

        let is_required = self
            .ethereum_l1
            .execution_layer
            .is_lookahead_required()
            .await?;

        if is_required {
            self.update_preconfer_lookahead_for_epoch().await?;
            if self
                .lookahead_preconfer_addresses
                .iter()
                .all(|addr| *addr == PRECONFER_ADDRESS_ZERO)
            {
                debug!("Lookahead is empty, force pushing");
                match self.ethereum_l1.force_push_lookahead().await {
                    Ok(_) => {
                        debug!("Force pushed lookahead");
                    }
                    Err(err) => {
                        if err.to_string().contains("AlreadyKnown") {
                            debug!("Force push lookahead already known");
                        } else {
                            error!("Failed to force push lookahead: {}", err);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[cfg(feature = "use_mock")]
mod tests {
    use super::*;
    use crate::ethereum_l1::{consensus_layer::ConsensusLayer, slot_clock::SlotClock};

    use mockall_double::double;

    #[double]
    use crate::ethereum_l1::execution_layer::ExecutionLayer;

    #[tokio::test]
    async fn test_get_status() {
        let mut execution_layer = ExecutionLayer::default();
        execution_layer
            .expect_get_lookahead_preconfer_addresses_for_epoch()
            .returning(|_| {
                Ok(vec![[1u8; 20], [1u8; 20]]
                    .into_iter()
                    .chain(std::iter::repeat([0u8; 20]).take(30))
                    .collect())
            });
        execution_layer
            .expect_get_lookahead_preconfer_buffer()
            .returning(|| Ok(create_lookahead_buffer()));
        let mut operator = create_operator(0, execution_layer).unwrap();
        operator
            .update_preconfer_lookahead_for_epoch()
            .await
            .unwrap();
        let status = operator.get_status(32).await.unwrap();
        assert_eq!(status, Status::Preconfer);

        let status = operator.get_status(33).await.unwrap();
        assert_eq!(status, Status::PreconferAndProposer);

        let status = operator.get_status(34).await.unwrap();
        assert_eq!(status, Status::None);
    }

    #[tokio::test]
    async fn test_get_status_last_slot_preconfer_and_proposer() {
        let mut execution_layer = ExecutionLayer::default();
        execution_layer
            .expect_get_lookahead_preconfer_addresses_for_epoch()
            .returning(|epoch_begin_timestamp| {
                if epoch_begin_timestamp == 0 {
                    Ok(vec![[1u8; 20]; 32])
                } else {
                    Ok(vec![[0u8; 20]; 32])
                }
            });
        execution_layer
            .expect_get_lookahead_preconfer_buffer()
            .returning(|| Ok(create_lookahead_buffer()));

        let mut operator = create_operator(0, execution_layer).unwrap();
        operator
            .update_preconfer_lookahead_for_epoch()
            .await
            .unwrap();
        let status = operator.get_status(31).await.unwrap();
        assert_eq!(status, Status::PreconferAndProposer);
    }

    #[tokio::test]
    async fn test_get_status_last_slot_preconfer() {
        let mut execution_layer = ExecutionLayer::default();
        execution_layer
            .expect_get_lookahead_preconfer_addresses_for_epoch()
            .returning(|epoch_begin_timestamp| {
                if epoch_begin_timestamp == 0 {
                    Ok(vec![[1u8; 20]; 32])
                } else {
                    Ok(vec![[1u8; 20]]
                        .into_iter()
                        .chain(std::iter::repeat([0u8; 20]).take(31))
                        .collect())
                }
            });
        execution_layer
            .expect_get_lookahead_preconfer_buffer()
            .returning(|| Ok(create_lookahead_buffer()));

        let mut operator = create_operator(0, execution_layer).unwrap();
        operator
            .update_preconfer_lookahead_for_epoch()
            .await
            .unwrap();
        let status = operator.get_status(31).await.unwrap();
        assert_eq!(status, Status::Preconfer);
    }

    fn create_operator(
        epoch: Epoch,
        mut execution_layer: ExecutionLayer,
    ) -> Result<Operator, Error> {
        execution_layer
            .expect_get_preconfer_address()
            .returning(|| PreconferAddress::from([1u8; 20]));
        let ethereum_l1 = Arc::new(EthereumL1 {
            slot_clock: Arc::new(SlotClock::new(0, 12, 12, 32, 3)),
            consensus_layer: ConsensusLayer::new("http://localhost:5052").unwrap(),
            execution_layer,
        });

        Operator::new(ethereum_l1, epoch)
    }

    fn create_lookahead_buffer() -> [PreconfTaskManager::LookaheadBufferEntry; 64] {
        use alloy::primitives::Address;

        let mut buffer = vec![];
        for _ in 0..64 {
            buffer.push(PreconfTaskManager::LookaheadBufferEntry {
                isFallback: false,
                timestamp: 0,
                prevTimestamp: 0,
                preconfer: Address::from([0u8; 20]),
            });
        }
        buffer
            .try_into()
            .unwrap_or_else(|_| panic!("Failed to convert buffer to array"))
    }
}
