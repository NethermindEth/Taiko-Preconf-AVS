use alloy::providers::DynProvider;
use catalyst_common::ethereum_l1::extension::ELExtension;

pub struct ExecutionLayer {
    provider: DynProvider,
}

impl ELExtension for ExecutionLayer {
    fn new(provider: DynProvider) -> Self {
        Self { provider }
    }
}

impl ExecutionLayer {
    fn get_operator_for_current_epoch() {}
}
