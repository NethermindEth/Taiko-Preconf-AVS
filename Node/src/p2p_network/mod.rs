use tokio::sync::mpsc::{Receiver, Sender};
use tracing::info;

pub struct AVSp2p {
    node_tx: Sender<String>,
    avs_p2p_rx: Receiver<String>,
}

impl AVSp2p {
    pub fn new(node_tx: Sender<String>, avs_p2p_rx: Receiver<String>) -> Self {
        AVSp2p {
            node_tx,
            avs_p2p_rx,
        }
    }

    // Consumes self and fires up threads
    pub fn start(mut self) {
        info!("Starting P2P network");

        //TODO for initial testing
        let node_tx = self.node_tx.clone();
        tokio::spawn(async move {
            loop {
                node_tx
                    .send("Hello from avs p2p!".to_string())
                    .await
                    .unwrap();
                tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
            }
        });

        tokio::spawn(async move {
            while let Some(message) = self.avs_p2p_rx.recv().await {
                tracing::debug!("AVS p2p received: {}", message);
            }
        });
    }
}
