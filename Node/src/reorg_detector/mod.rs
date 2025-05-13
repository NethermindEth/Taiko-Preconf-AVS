use alloy::primitives::Address;
use anyhow::Error;
use batch_proposed_receiver::BatchProposedEventReceiver;
use l2_block_receiver::L2BlockReceiver;
use std::str::FromStr;

mod batch_proposed;
mod batch_proposed_receiver;
mod l2_block_receiver;

pub struct ReorgDetector {
    ws_l1_rpc_url: String,
    ws_l2_rpc_url: String,
    taiko_inbox: Address,
}

impl ReorgDetector {
    pub fn new(
        ws_l1_rpc_url: String,
        ws_l2_rpc_url: String,
        taiko_inbox: String,
    ) -> Result<Self, Error> {
        tracing::debug!("Creating ReorgDetector with WS URL: {}, L2 WS URL: {}, TaikoInbox: {}", ws_l1_rpc_url, ws_l2_rpc_url, taiko_inbox);

        let taiko_inbox = Address::from_str(taiko_inbox.as_str()).unwrap();

        Ok(Self {
            ws_l1_rpc_url,
            ws_l2_rpc_url,
            taiko_inbox,
        })
    }

    pub async fn start(&self) -> Result<(), Error> {
        let receiver =
            BatchProposedEventReceiver::new(self.ws_l1_rpc_url.clone(), self.taiko_inbox).await?;
        let _ = receiver.start();

        let l2_block_receiver = L2BlockReceiver::new(self.ws_l2_rpc_url.clone());
        let _ = l2_block_receiver.start();

        tracing::debug!("ReorgDetector started");
        Ok(())
    }
}
