use alloy::providers::DynProvider;
use catalyst_node::ethereum_l1::extension::ELExtension;

pub struct ExecutionLayer {
    provider: DynProvider,
}

impl ELExtension for ExecutionLayer {
    fn new(provider: DynProvider) -> Self {
        Self { provider }
    }

    // fn register() {}
}

// impl Send for ExecutionLayer {}
// impl Sync for ExecutionLayer {}
