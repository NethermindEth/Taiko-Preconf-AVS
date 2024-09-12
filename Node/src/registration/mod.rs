use crate::ethereum_l1::EthereumL1;
use anyhow::Error;

mod tests;
pub struct Registration {
    ethereum_l1: EthereumL1,
}

impl Registration {
    pub fn new(ethereum_l1: EthereumL1) -> Self {
        Self { ethereum_l1 }
    }

    pub async fn register(&self) -> Result<(), Error> {
        let registered_filter = self
            .ethereum_l1
            .execution_layer
            .subscribe_to_registered_event()
            .await?;

        self.ethereum_l1
            .execution_layer
            .register_preconfer()
            .await?;

        self.ethereum_l1
            .execution_layer
            .wait_for_the_registered_event(registered_filter)
            .await?;

        Ok(())
    }

    pub async fn add_validator(&self) -> Result<(), Error> {
        let validator_added_filter = self
            .ethereum_l1
            .execution_layer
            .subscribe_to_validator_added_event()
            .await?;

        self.ethereum_l1.execution_layer.add_validator().await?;

        self.ethereum_l1
            .execution_layer
            .wait_for_the_validator_added_event(validator_added_filter)
            .await?;

        Ok(())
    }
}
