pub struct ExecutionLayerInner {
    chain_id: u64,
}

impl ExecutionLayerInner {
    pub fn new(chain_id: u64) -> Self {
        Self { chain_id }
    }

    pub fn chain_id(&self) -> u64 {
        self.chain_id
    }
}
