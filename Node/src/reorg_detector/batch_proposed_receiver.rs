use alloy::{
    primitives::Address,
    providers::{ProviderBuilder, WsConnect},
};
use anyhow::Error;
use futures_util::StreamExt;
use std::sync::Arc;
use tracing::{error, info};

use crate::reorg_detector::batch_proposed::{BatchProposed, TaikoEvents};

use super::batch_proposed::EventSubscriptionBatchProposed;

type WsProvider = alloy::providers::fillers::FillProvider<
    alloy::providers::fillers::JoinFill<
        alloy::providers::Identity,
        alloy::providers::fillers::JoinFill<
            alloy::providers::fillers::GasFiller,
            alloy::providers::fillers::JoinFill<
                alloy::providers::fillers::BlobGasFiller,
                alloy::providers::fillers::JoinFill<
                    alloy::providers::fillers::NonceFiller,
                    alloy::providers::fillers::ChainIdFiller,
                >,
            >,
        >,
    >,
    alloy::providers::RootProvider,
>;

pub struct BatchProposedEventReceiver {
    provider_ws: Arc<WsProvider>,
    ws_rpc_url: String,
    taiko_inbox: Address,
    //node_tx: Sender<BatchProposed>,
}

impl BatchProposedEventReceiver {
    pub async fn new(ws_rpc_url: String, taiko_inbox: Address) -> Result<Self, Error> {
        let ws = WsConnect::new(ws_rpc_url.to_string());

        let provider_ws: Arc<WsProvider> = Arc::new(ProviderBuilder::new().on_ws(ws).await?);

        Ok(BatchProposedEventReceiver {
            provider_ws,
            ws_rpc_url,
            taiko_inbox,
        })
    }

    pub fn start(&self) {
        info!("Starting batch proposed event receiver");
        let provider_ws = self.provider_ws.clone();
        let taiko_inbox = self.taiko_inbox.clone();
        tokio::spawn(async move {
            BatchProposedEventReceiver::check_for_events(provider_ws, taiko_inbox).await;
        });
    }

    pub async fn subscribe_to_batch_proposed_event(
        provider_ws: Arc<WsProvider>,
        taiko_inbox: Address,
    ) -> Result<EventSubscriptionBatchProposed, Error> {
        let taiko_events = TaikoEvents::new(taiko_inbox, &provider_ws);

        let batch_proposed_filter = taiko_events.BatchProposed_filter().subscribe().await?;
        tracing::debug!("Subscribed to batch proposed event");

        Ok(EventSubscriptionBatchProposed(batch_proposed_filter))
    }

    async fn check_for_events(provider_ws: Arc<WsProvider>, taiko_inbox: Address) {
        let event_subscription =
            match BatchProposedEventReceiver::subscribe_to_batch_proposed_event(
                provider_ws,
                taiko_inbox,
            )
            .await
            {
                Ok(event_stream) => event_stream,
                Err(e) => {
                    error!("Error subscribing to batch proposed event: {:?}", e);
                    return;
                }
            };

        let mut stream = event_subscription.0.into_stream();
        loop {
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
                            }
                        }
                    }
                    Err(e) => {
                        error!("Error receiving batch proposed event: {:?}", e);
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    }
                },
                None => {
                    error!("No batch proposed event received, stream closed");
                    // TODO: recreate a stream
                    return;
                }
            }
        }
    }
}
