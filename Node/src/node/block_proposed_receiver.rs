use crate::taiko::Taiko;
use crate::utils::block_proposed::BlockProposed;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tracing::{error, info};

pub struct BlockProposedEventReceiver {
    taiko: Arc<Taiko>,
    node_tx: Sender<BlockProposed>,
}

impl BlockProposedEventReceiver {
    pub fn new(taiko: Arc<Taiko>, node_tx: Sender<BlockProposed>) -> Self {
        Self { taiko, node_tx }
    }

    pub fn start(receiver: Self) {
        tokio::spawn(async move {
            receiver.check_for_events().await;
        });
    }

    pub async fn check_for_events(&self) {
        loop {
            let block_proposed_event = self.taiko.wait_for_block_proposed_event().await;
            match block_proposed_event {
                Ok(block_proposed) => {
                    info!(
                        "Received block proposed event for block: {}",
                        block_proposed.block_id
                    );
                    if let Err(e) = self.node_tx.send(block_proposed).await {
                        error!("Error sending block proposed event by channel: {:?}", e);
                    }
                }
                Err(e) => {
                    error!("Error receiving block proposed event: {:?}", e);
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                }
            }
        }
    }
}
