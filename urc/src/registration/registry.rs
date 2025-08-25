use std::sync::Arc;

use crate::l1::execution_layer::ExecutionLayer;
use common::ethereum_l1::EthereumL1;

pub struct Registry {
    ethereum_l1: Arc<EthereumL1<ExecutionLayer>>,
}

impl Registry {
    pub fn new(ethereum_l1: Arc<EthereumL1<ExecutionLayer>>) -> Self {
        Self { ethereum_l1 }
    }

    //TODO pull logs for all registration events and take tx hash from each to get
    // all registration transactions to read their calldata
    // BTW, event listner will be needed later to update the mapping.
    async fn pull_reistriation_events(&self) {
        // let filter = Filter::new()
        //     .address(self.ethereum_l1.registry_address)
        //     .event_signature(self.ethereum_l1.registry_address);

        // let logs = self.ethereum_l1.execution_layer.get_logs(filter).await;
    }
}
