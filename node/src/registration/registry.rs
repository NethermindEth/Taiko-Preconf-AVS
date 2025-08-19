

struct Registry {
    ethereum_l1: Arc<EthereumL1>,
}

impl Registry {
    pub fn new(ethereum_l1: Arc<EthereumL1>) -> Self {
        Self { ethereum_l1 }
    }

    //TODO pull logs for all registration events and take tx hash from each to get
    // all registration transactions to read their calldata
    // BTW, event listner will be needed later to update the mapping.
    fn pull_reistriation_events(&self)
}