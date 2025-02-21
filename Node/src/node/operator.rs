use crate::ethereum_l1::EthereumL1;
use anyhow::Error;
use std::sync::Arc;

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
        Ok(Self { ethereum_l1 })
    }

    pub async fn get_status(&mut self) -> Result<Status, Error> {
        let slot = self.ethereum_l1.slot_clock.get_current_slot_of_epoch()?;
        let current_operator = match slot {
            0 => {
                self.ethereum_l1
                    .execution_layer
                    .get_operator_for_next_epoch()
                    .await?
            }
            _ => {
                self.ethereum_l1
                    .execution_layer
                    .get_operator_for_current_epoch()
                    .await?
            }
        };
        if current_operator == self.ethereum_l1.execution_layer.get_preconfer_address() {
            return Ok(Status::Preconfer);
        }
        Ok(Status::None)
    }
}
