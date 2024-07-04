use crate::utils::rpc_client::RpcClient;

pub struct MevBoost {
    _rpc_client: RpcClient,
}

impl MevBoost {
    pub fn new(rpc_url: &str) -> Self {
        let rpc_client = RpcClient::new(rpc_url);
        Self {
            _rpc_client: rpc_client,
        }
    }

    pub fn send_transaction(&self, _tx: &[u8], _validator_index: u64, _slot: u64) {
        //TODO: implement
    }
}
