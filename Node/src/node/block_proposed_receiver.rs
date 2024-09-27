use crate::ethereum_l1::{block_proposed::BlockProposedV2, EthereumL1};
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tracing::{error, info};

pub struct BlockProposedEventReceiver {
    ethereum_l1: Arc<EthereumL1>,
    node_tx: Sender<BlockProposedV2>,
}

impl BlockProposedEventReceiver {
    pub fn new(ethereum_l1: Arc<EthereumL1>, node_tx: Sender<BlockProposedV2>) -> Self {
        Self {
            ethereum_l1,
            node_tx,
        }
    }

    pub fn start(receiver: Self) {
        tokio::spawn(async move {
            receiver.check_for_events().await;
        });
    }

    async fn check_for_events(self) {
        let event_poller = match self
            .ethereum_l1
            .execution_layer
            .subscribe_to_block_proposed_event()
            .await
        {
            Ok(event_stream) => event_stream,
            Err(e) => {
                error!("Error subscribing to block proposed event: {:?}", e);
                return;
            }
        };

        let mut stream = event_poller.0.into_stream();
        loop {
            match stream.next().await {
                Some(log) => match log {
                    Ok(log) => {
                        let block_proposed = log.0;
                        info!(
                            "Received block proposed event for block: {}",
                            block_proposed.blockId
                        );
                        match BlockProposedV2::new(block_proposed) {
                            Ok(block_proposed) => {
                                if let Err(e) = self.node_tx.send(block_proposed).await {
                                    error!(
                                        "Error sending block proposed event by channel: {:?}",
                                        e
                                    );
                                }
                            }
                            Err(e) => {
                                error!("Error creating block proposed event: {:?}", e);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error receiving block proposed event: {:?}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    }
                },
                None => {
                    error!("No block proposed event received, stream closed");
                    // TODO: recreate a stream in this case?
                }
            }
        }
    }
}
