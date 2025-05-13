use alloy::{
    primitives::Address,
    providers::{ProviderBuilder, WsConnect},
};
use anyhow::Error;
use futures_util::StreamExt;
//cd ..use std::sync::Arc;
use tracing::{error, info};
use tokio::time::{sleep, Duration};

use crate::reorg_detector::batch_proposed::{BatchProposed, TaikoEvents};

use super::batch_proposed::EventSubscriptionBatchProposed;

const SLEEP_DURATION: u64 = 15;

pub struct BatchProposedEventReceiver {
    ws_rpc_url: String,
    taiko_inbox: Address,
    //node_tx: Sender<BatchProposed>,
}

impl BatchProposedEventReceiver {
    pub async fn new(ws_rpc_url: String, taiko_inbox: Address) -> Result<Self, Error> {
        Ok(BatchProposedEventReceiver {
            ws_rpc_url,
            taiko_inbox,
        })
    }

    pub fn start(&self) {
        info!("Starting batch proposed event receiver");
        let ws_rpc_url = self.ws_rpc_url.clone();
        let taiko_inbox = self.taiko_inbox.clone();
        tokio::spawn(async move {
            BatchProposedEventReceiver::check_for_events(ws_rpc_url, taiko_inbox).await;
        });
    }

    async fn check_for_events(ws_rpc_url: String, taiko_inbox: Address) {
        loop {
            let ws = WsConnect::new(ws_rpc_url.to_string());

            let Ok(provider_ws) = ProviderBuilder::new().on_ws(ws.clone()).await else {
                tracing::error!("Failed to create WebSocket provider");
                sleep(Duration::from_secs(SLEEP_DURATION)).await;
                continue;
            };

            let taiko_events = TaikoEvents::new(taiko_inbox, &provider_ws);

            let Ok(batch_proposed_filter) = taiko_events.BatchProposed_filter().subscribe().await
            else {
                tracing::error!("Failed to create BatchProposed_filter");
                sleep(Duration::from_secs(SLEEP_DURATION)).await;
                continue;
            };
            tracing::debug!("Subscribed to batch proposed event");

            let event_subscription = EventSubscriptionBatchProposed(batch_proposed_filter);

            let mut stream = event_subscription.0.into_stream();

            match stream.next().await {
                Some(log) => match log {
                    Ok(log) => {
                        let batch_proposed = log.0;
                        info!(
                            "Received batch proposed event for block: {}",
                            batch_proposed.info.lastBlockId
                        );
                        match BatchProposed::new(batch_proposed) {
                            Ok(batch_proposed) => {
                                info!(
                                    "BatchProposed lastBlockId: {}, blocks len: {}",
                                    batch_proposed.event_data().info.lastBlockId,
                                    batch_proposed.event_data().info.blocks.len()
                                );
                                /*if let Err(e) = self.node_tx.send(batch_proposed).await {
                                    error!(
                                        "Error sending batch proposed event by channel: {:?}",
                                        e
                                    );
                                }*/
                            }
                            Err(e) => {
                                error!("Error creating batch proposed event: {:?}", e);
                                tokio::time::sleep(std::time::Duration::from_secs(SLEEP_DURATION))
                                    .await;
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error receiving batch proposed event: {:?}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(SLEEP_DURATION)).await;
                        continue;
                    }
                },
                None => {
                    error!("No batch proposed event received, stream closed");
                    // TODO: recreate a stream
                    //return;
                    sleep(Duration::from_secs(SLEEP_DURATION)).await;
                    continue;
                }
            }
        }
    }
}
