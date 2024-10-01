use alloy::{contract::EventSubscription, sol};
use anyhow::Error;

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    TaikoEvents,
    "src/ethereum_l1/abi/TaikoEvents.json"
);

pub struct BlockProposed {
    event_data: TaikoEvents::BlockProposed,
    block_id: u64,
}

impl BlockProposed {
    pub fn new(event_data: TaikoEvents::BlockProposed) -> Result<Self, Error> {
        let block_id = event_data.blockId.try_into()?;
        Ok(Self {
            event_data,
            block_id,
        })
    }

    pub fn block_id(&self) -> u64 {
        self.block_id
    }
    pub fn event_data(&self) -> &TaikoEvents::BlockProposed {
        &self.event_data
    }
}

pub struct EventSubscriptionBlockProposed(pub EventSubscription<TaikoEvents::BlockProposed>);
