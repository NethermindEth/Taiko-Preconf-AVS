use alloy::providers::{Provider, ProviderBuilder, WsConnect};
use anyhow::Error;

use tokio::time::{sleep, Duration};

pub struct L2BlockReceiver {
    ws_rpc_url: String,
}

impl L2BlockReceiver {
    pub fn new(ws_rpc_url: String) -> Self {
        Self { ws_rpc_url }
    }

    pub fn start(&self) -> Result<(), Error> {
        let rpc_url = self.ws_rpc_url.clone();
        tokio::spawn(async move {
            loop {
                let ws = WsConnect::new(rpc_url.to_string());

                let Ok(provider_ws) = ProviderBuilder::new().on_ws(ws.clone()).await else {
                    tracing::error!("Failed to create WebSocket provider");
                    sleep(Duration::from_secs(5)).await;
                    continue;
                };

                // Subscribe to block headers
                let Ok(mut subscription) = provider_ws.subscribe_blocks().await else {
                    tracing::error!("Failed to subscribe to taiko new blocks");
                    sleep(Duration::from_secs(5)).await;
                    continue;
                };

                while let Ok(header) = subscription.recv().await {
                    tracing::info!(
                        "Received taiko block number: {}, hash: {}",
                        header.number,
                        header.hash
                    );
                }

                tracing::warn!("Subscription to new blocks closed, retrying...");
            }
        });

        Ok(())
    }
}
