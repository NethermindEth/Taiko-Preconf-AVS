use crate::ethereum_l1::{
    execution_layer::{EventPollerLookaheadUpdated, PreconfTaskManager},
    EthereumL1,
};
use anyhow::Error;
use futures_util::StreamExt;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use tracing::{debug, error};

pub struct LookaheadUpdated {
    pub lookahead_params: Vec<PreconfTaskManager::LookaheadSetParam>,
}

#[derive(Clone)]
pub struct LookaheadUpdatedEventReceiver {
    ethereum_l1: Arc<EthereumL1>,
    node_tx: Sender<LookaheadUpdated>,
}

impl LookaheadUpdatedEventReceiver {
    pub fn new(
        ethereum_l1: Arc<EthereumL1>,
        node_tx: Sender<LookaheadUpdated>,
    ) -> Result<Self, Error> {
        Ok(Self {
            ethereum_l1,
            node_tx,
        })
    }

    pub fn start(self) {
        let ethereum_l1 = self.ethereum_l1.clone();
        let node_tx = self.node_tx.clone();
        tokio::spawn(async move {
            Self::check_for_events(ethereum_l1, node_tx).await;
        });
    }

    pub async fn check_for_events(ethereum_l1: Arc<EthereumL1>, node_tx: Sender<LookaheadUpdated>) {
        let event_poller = match ethereum_l1
            .execution_layer
            .subscribe_to_lookahead_updated_event()
            .await
        {
            Ok(event_stream) => event_stream,
            Err(e) => {
                error!("Error subscribing to lookahead updated event: {:?}", e);
                return;
            }
        };

        let mut stream = event_poller.0.into_stream();
        loop {
            match stream.next().await {
                Some(log) => match log {
                    Ok(log) => {
                        let lookahead_params = log.0._0;
                        debug!(
                            "Received lookahead updated event with {} params.",
                            lookahead_params.len()
                        );
                        if let Err(e) = node_tx.send(LookaheadUpdated { lookahead_params }).await {
                            error!("Error sending lookahead updated event by channel: {:?}", e);
                        }
                    }
                    Err(e) => {
                        error!("Error receiving lookahead updated event: {:?}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    }
                },
                None => {
                    error!("No lookahead updated event received, stream closed");
                    // TODO: recreate a stream in this case?
                }
            }
        }
    }
}
