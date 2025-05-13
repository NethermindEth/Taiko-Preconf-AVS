use alloy::primitives::B256;
use alloy::providers::{Provider, ProviderBuilder, WsConnect};
use anyhow::Error;
use tokio::{
    sync::mpsc::Sender,
    time::{sleep, Duration},
};
use tracing::{error, info, warn};

const SLEEP_DURATION: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct L2BlockInfo {
    pub block_number: u64,
    pub block_hash: B256,
}

pub struct L2BlockReceiver {
    ws_rpc_url: String,
    l2_block_info_tx: Sender<L2BlockInfo>,
}

impl L2BlockReceiver {
    pub fn new(ws_rpc_url: String, l2_block_info_tx: Sender<L2BlockInfo>) -> Self {
        Self {
            ws_rpc_url,
            l2_block_info_tx,
        }
    }

    pub fn start(&self) -> Result<(), Error> {
        let rpc_url = self.ws_rpc_url.clone();
        let l2_block_info_tx = self.l2_block_info_tx.clone();

        tokio::spawn(async move {
            loop {
                if let Err(e) = Self::listen_for_blocks(&rpc_url, l2_block_info_tx.clone()).await {
                    error!("Error in block listener: {:?}", e);
                    sleep(SLEEP_DURATION).await;
                }
            }
        });

        Ok(())
    }

    async fn listen_for_blocks(
        rpc_url: &str,
        l2_block_info_tx: Sender<L2BlockInfo>,
    ) -> Result<(), Error> {
        let ws = WsConnect::new(rpc_url.to_string());

        let provider_ws = ProviderBuilder::new().on_ws(ws).await.map_err(|e| {
            error!("Failed to create WebSocket provider: {:?}", e);
            e
        })?;

        let mut subscription = provider_ws.subscribe_blocks().await.map_err(|e| {
            error!("Failed to subscribe to new blocks: {:?}", e);
            e
        })?;

        info!("Subscribed to L2 block headers");

        while let Ok(header) = subscription.recv().await {
            let block_info = L2BlockInfo {
                block_number: header.number,
                block_hash: header.hash,
            };

            info!(
                "Received Taiko block number: {}, hash: {}",
                block_info.block_number, block_info.block_hash
            );

            if let Err(e) = l2_block_info_tx.send(block_info).await {
                return Err(anyhow::anyhow!("Failed to send block info: {:?}", e));
            }
        }

        warn!("Block subscription stream ended or failed");
        Ok(())
    }
}
