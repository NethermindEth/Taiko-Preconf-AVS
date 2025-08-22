use alloy::providers::DynProvider;
use catalyst_common::ethereum_l1::{
    execution_layer_inner::ExecutionLayerInner, extension::ELExtension,
};
use std::sync::Arc;

pub struct ExecutionLayer {
    inner: Arc<ExecutionLayerInner>,
    provider: DynProvider,
}

impl ELExtension for ExecutionLayer {
    fn new(inner: Arc<ExecutionLayerInner>, provider: DynProvider) -> Self {
        Self { inner, provider }
    }
}

impl ExecutionLayer {
    fn register(&self) {
        let chain_id = self.inner.chain_id();
    }
}
