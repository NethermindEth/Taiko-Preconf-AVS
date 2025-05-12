use alloy::{contract::EventSubscription, sol};
use anyhow::Error;

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    TaikoEvents,
    "src/ethereum_l1/abi/TaikoEvents.json"
);

pub struct BatchProposed {
    event_data: TaikoEvents::BatchProposed,
    last_block_id: u64,
}

impl BatchProposed {
    pub fn new(event_data: TaikoEvents::BatchProposed) -> Result<Self, Error> {
        let last_block_id = event_data.info.lastBlockId.try_into()?;
        Ok(Self {
            event_data,
            last_block_id,
        })
    }

    pub fn last_block_id(&self) -> u64 {
        self.last_block_id
    }
    pub fn event_data(&self) -> &TaikoEvents::BatchProposed {
        &self.event_data
    }
}

pub struct EventSubscriptionBatchProposed(pub EventSubscription<TaikoEvents::BatchProposed>);
