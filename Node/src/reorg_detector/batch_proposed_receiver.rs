use alloy::{
    primitives::Address,
    providers::{ProviderBuilder, WsConnect},
};
use anyhow::Error;
use futures_util::StreamExt;
use tokio::{
    select,
    sync::mpsc::Sender,
    time::{sleep, Duration},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::reorg_detector::batch_proposed::{BatchProposed, TaikoEvents};

const SLEEP_DURATION: Duration = Duration::from_secs(15);

pub struct BatchProposedEventReceiver {
    ws_rpc_url: String,
    taiko_inbox: Address,
    batch_proposed_tx: Sender<BatchProposed>,
    cancel_token: CancellationToken,
}

impl BatchProposedEventReceiver {
    pub async fn new(
        ws_rpc_url: String,
        taiko_inbox: Address,
        batch_proposed_tx: Sender<BatchProposed>,
        cancel_token: CancellationToken,
    ) -> Result<Self, Error> {
        Ok(Self {
            ws_rpc_url,
            taiko_inbox,
            batch_proposed_tx,
            cancel_token,
        })
    }

    pub fn start(&self) {
        info!("Starting BatchProposed event receiver");
        let ws_rpc_url = self.ws_rpc_url.clone();
        let taiko_inbox = self.taiko_inbox;
        let batch_proposed_tx = self.batch_proposed_tx.clone();
        let cancel_token = self.cancel_token.clone();

        tokio::spawn(async move {
            Self::check_for_events(ws_rpc_url, taiko_inbox, batch_proposed_tx, cancel_token).await;
        });
    }

    async fn check_for_events(
        ws_rpc_url: String,
        taiko_inbox: Address,
        batch_proposed_tx: Sender<BatchProposed>,
        cancel_token: CancellationToken,
    ) {
        loop {
            if cancel_token.is_cancelled() {
                info!("BatchProposedEventReceiver: cancellation requested, exiting loop");
                break;
            }

            let ws = WsConnect::new(ws_rpc_url.clone());

            let provider_ws = match ProviderBuilder::new().on_ws(ws).await {
                Ok(provider) => provider,
                Err(e) => {
                    error!("Failed to create WebSocket provider: {:?}", e);
                    sleep(SLEEP_DURATION).await;
                    continue;
                }
            };

            let taiko_events = TaikoEvents::new(taiko_inbox, &provider_ws);

            let batch_proposed_filter = match taiko_events.BatchProposed_filter().subscribe().await {
                Ok(filter) => filter,
                Err(e) => {
                    error!("Failed to subscribe to BatchProposed_filter: {:?}", e);
                    sleep(SLEEP_DURATION).await;
                    continue;
                }
            };

            tracing::debug!("Subscribed to BatchProposed event");
            let mut stream = batch_proposed_filter.into_stream();

            loop {
                select! {
                    _ = cancel_token.cancelled() => {
                        info!("BatchProposedEventReceiver: cancellation received, stopping event loop");
                        return;
                    }
                    result = stream.next() => {
                        match result {
                            Some(Ok(log)) => {
                                let raw_event = log.0;
                                match BatchProposed::new(raw_event) {
                                    Ok(batch_proposed) => {
                                        info!(
                                            "Parsed BatchProposed: lastBlockId = {}, blocks = {}",
                                            batch_proposed.event_data().info.lastBlockId,
                                            batch_proposed.event_data().info.blocks.len()
                                        );

                                        if let Err(e) = batch_proposed_tx.send(batch_proposed).await {
                                            error!("Failed to send BatchProposed event: {:?}", e);
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to parse BatchProposed event: {:?}", e);
                                        break;
                                    }
                                }
                            }
                            Some(Err(e)) => {
                                error!("Error in BatchProposed event stream: {:?}", e);
                                break;
                            }
                            None => {
                                warn!("Stream ended");
                                break;
                            }
                        }
                    }
                }
            }

            warn!("BatchProposed stream ended or errored; reconnecting...");
            sleep(SLEEP_DURATION).await;
        }
    }
}
