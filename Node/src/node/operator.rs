use crate::{
    ethereum_l1::EthereumL1,
    utils::types::*,
};
use anyhow::Error;
use std::sync::Arc;
use tracing::{debug, error};

pub struct Operator {
    ethereum_l1: Arc<EthereumL1>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Status {
    None,
    Preconfer,
    PreconferAndProposer, // has to force include transactions
}

impl Operator {
    pub fn new(ethereum_l1: Arc<EthereumL1>) -> Result<Self, Error> {
        Ok(Self {
            ethereum_l1,
        })
    }

    pub async fn get_status(&mut self) -> Result<Status, Error> {
        let slot = self.ethereum_l1.slot_clock.get_current_slot_of_epoch()?;
        let current_operator = match slot {
            0 => self.ethereum_l1.execution_layer.get_operator_for_next_epoch().await?,
            _ => self.ethereum_l1.execution_layer.get_operator_for_current_epoch().await?,
        };
        if current_operator == self.ethereum_l1.execution_layer.get_preconfer_address() {
            return Ok(Status::Preconfer);
        }
        Ok(Status::None)
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

    fn create_lookahead_buffer() -> [PreconfTaskManager::LookaheadBufferEntry; 128] {
        use alloy::primitives::Address;

        let mut buffer = vec![];
        for _ in 0..128 {
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
