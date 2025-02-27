use alloy::{contract::EventSubscription, sol};
use anyhow::Error;

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    TaikoEvents,
    "src/ethereum_l1/abi/TaikoEvents.json"
);

pub struct BlockProposedV2 {
    event_data: TaikoEvents::BlockProposedV2,
    block_id: u64,
}

impl BlockProposedV2 {
    pub fn new(event_data: TaikoEvents::BlockProposedV2) -> Result<Self, Error> {
        let block_id = event_data.blockId.try_into()?;
        Ok(Self {
            event_data,
            block_id,
        })
    }

    pub fn block_id(&self) -> u64 {
        self.block_id
    }
    pub fn event_data(&self) -> &TaikoEvents::BlockProposedV2 {
        &self.event_data
    }
}

pub struct EventSubscriptionBlockProposedV2(pub EventSubscription<TaikoEvents::BlockProposedV2>);
