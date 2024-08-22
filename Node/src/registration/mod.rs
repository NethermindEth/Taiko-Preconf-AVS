use crate::ethereum_l1::EthereumL1;
use anyhow::Error;

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
            .watch_for_registered_event()
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
}
