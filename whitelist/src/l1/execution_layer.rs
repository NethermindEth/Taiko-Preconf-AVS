use alloy::providers::DynProvider;
use common::ethereum_l1::{execution_layer_inner::ExecutionLayerInner, extension::ELExtension};
use std::sync::Arc;

pub struct EthereumL1Config {}

pub struct ExecutionLayer {
    inner: Arc<ExecutionLayerInner>,
    provider: DynProvider,
    config: EthereumL1Config,
}

impl ELExtension for ExecutionLayer {
    type Config = EthereumL1Config;
    fn new(
        inner: Arc<ExecutionLayerInner>,
        provider: DynProvider,
        config: EthereumL1Config,
    ) -> Self {
        Self {
            inner,
            provider,
            config,
        }
    }
}

impl ExecutionLayer {
    fn get_operator_for_current_epoch() {}
}
