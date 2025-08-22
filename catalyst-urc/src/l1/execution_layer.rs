use alloy::{
    primitives::{Address, Log},
    providers::DynProvider,
    rpc::types::Filter,
};
use catalyst_common::ethereum_l1::{
    execution_layer_inner::ExecutionLayerInner, extension::ELExtension,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct L1ContractAddresses {
    pub registry_address: Address,
}

#[derive(Clone)]
pub struct EthereumL1Config {
    contract_addresses: L1ContractAddresses,
}

pub struct ExecutionLayer {
    inner: Arc<ExecutionLayerInner>,
    provider: DynProvider,
    config: EthereumL1Config,
}

impl ELExtension for ExecutionLayer {
    type Config = EthereumL1Config;
    fn new(inner: Arc<ExecutionLayerInner>, provider: DynProvider, config: Self::Config) -> Self {
        Self {
            inner,
            provider,
            config,
        }
    }
}

// potrzebuje address registrya
// chyba sobie tego traita from zaimplementuje, bo  jak inaczej to zrobić?

impl ExecutionLayer {
    async fn get_logs_for_register_method(&self) -> Vec<Log> {
        // let chain_id = self.inner.chain_id();
        // let registry_address = self.inner.registry_address();

        // let filter = Filter::new()
        //     .address(registry_address)
        //     .event_signature(registry_address);

        // let logs = self.provider.get_logs(&filter).await.unwrap();

        // logs

        vec![]
    }
}
