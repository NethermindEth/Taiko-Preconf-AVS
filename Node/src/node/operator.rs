use crate::{ethereum_l1::EthereumL1, utils::types::*};
use anyhow::Error;
use std::sync::Arc;

pub struct Operator {
    ethereum_l1: Arc<EthereumL1>,
    epoch_begin_timestamp: u64,
    lookahead_required_contract_called: bool,
    lookahead_preconfer_addresses: Vec<PreconferAddress>,
    lookahead_preconfer_addresses_next_epoch: Option<Vec<PreconferAddress>>,
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
        let l1_slots_per_epoch = ethereum_l1.slot_clock.get_slots_per_epoch();
        let epoch_begin_timestamp = ethereum_l1.slot_clock.get_epoch_begin_timestamp(epoch)?;
        Ok(Self {
            ethereum_l1,
            epoch_begin_timestamp,
            lookahead_required_contract_called: false,
            lookahead_preconfer_addresses: vec![],
            lookahead_preconfer_addresses_next_epoch: None,
            l1_slots_per_epoch,
        })
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
                .get_lookahead_preconfer_addresses_for_epoch(
                    self.epoch_begin_timestamp
                        + self.ethereum_l1.slot_clock.get_epoch_duration_secs(),
                )
                .await?;
            let address = lookahead_preconfer_addresses_next_epoch[0];
            self.lookahead_preconfer_addresses_next_epoch =
                Some(lookahead_preconfer_addresses_next_epoch);
            Ok(address)
        } else {
            Ok(self.lookahead_preconfer_addresses[(slot_mod_slots_per_epoch + 1) as usize])
        }
    }

    fn is_the_final_slot_to_preconf(&self, next_preconfer_address: PreconferAddress) -> bool {
        next_preconfer_address != self.ethereum_l1.execution_layer.get_preconfer_address()
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
        tracing::debug!(
            "Updating preconfer lookahead for epoch: {}",
            self.ethereum_l1
                .slot_clock
                .get_epoch_for_timestamp(self.epoch_begin_timestamp)?
        );

        if let Some(lookahead_preconfer_addresses_next_epoch) =
            self.lookahead_preconfer_addresses_next_epoch.take()
        {
            self.lookahead_preconfer_addresses = lookahead_preconfer_addresses_next_epoch;
        } else {
            self.lookahead_preconfer_addresses = self
                .ethereum_l1
                .execution_layer
                .get_lookahead_preconfer_addresses_for_epoch(self.epoch_begin_timestamp)
                .await?;
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
            slot_clock: Arc::new(SlotClock::new(0, 0, 12, 32)),
            consensus_layer: ConsensusLayer::new("http://localhost:5052").unwrap(),
            execution_layer,
        });

        Operator::new(ethereum_l1, epoch)
    }
}
