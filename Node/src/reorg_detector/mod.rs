use alloy::primitives::Address;
use anyhow::{anyhow, Error};
use batch_proposed::BatchProposed;
use batch_proposed_receiver::BatchProposedEventReceiver;
use l2_block_receiver::{L2BlockInfo, L2BlockReceiver};
use std::str::FromStr;
use tokio::sync::mpsc::{self, Receiver};
use tracing::{debug, info};

mod batch_proposed;
mod batch_proposed_receiver;
mod l2_block_receiver;

const MESSAGE_QUEUE_SIZE: usize = 20;

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
        debug!(
            "Creating ReorgDetector (L1: {}, L2: {}, Inbox: {})",
            ws_l1_rpc_url, ws_l2_rpc_url, taiko_inbox
        );

        let taiko_inbox = Address::from_str(&taiko_inbox)
            .map_err(|e| anyhow!("Invalid Taiko inbox address: {:?}", e))?;

        Ok(Self {
            ws_l1_rpc_url,
            ws_l2_rpc_url,
            taiko_inbox,
        })
    }

    /// Spawns the event listeners and the message handler.
    pub async fn start(&self) -> Result<(), Error> {
        debug!("Starting ReorgDetector");

        //BatchProposed events
        let (batch_proposed_tx, batch_proposed_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);
        let batch_receiver = BatchProposedEventReceiver::new(
            self.ws_l1_rpc_url.clone(),
            self.taiko_inbox,
            batch_proposed_tx,
        )
        .await?;
        batch_receiver.start();

        //L2 block headers
        let (l2_block_tx, l2_block_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);
        let l2_receiver = L2BlockReceiver::new(self.ws_l2_rpc_url.clone(), l2_block_tx);
        l2_receiver.start()?;

        //Message dispatcher
        tokio::spawn(Self::handle_incoming_messages(
            batch_proposed_rx,
            l2_block_rx,
        ));

        Ok(())
    }

    async fn handle_incoming_messages(
        mut batch_proposed_rx: Receiver<BatchProposed>,
        mut l2_block_rx: Receiver<L2BlockInfo>,
    ) {
        info!("ReorgDetector message loop running");

        loop {
            tokio::select! {
                Some(batch) = batch_proposed_rx.recv() => {
                    info!(
                        "BatchProposed event → lastBlockId = {}",
                        batch.event_data().info.lastBlockId
                    );
                }
                Some(block) = l2_block_rx.recv() => {
                    info!(
                        "L2 block → number: {}, hash: {}",
                        block.block_number, block.block_hash
                    );
                }
            }
        }
    }
}
