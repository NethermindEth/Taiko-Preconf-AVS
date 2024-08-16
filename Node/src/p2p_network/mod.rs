use p2p_network::network::{P2PNetwork, P2PNetworkConfig};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task;
use tracing::info;

pub struct AVSp2p {
    node_tx: Sender<Vec<u8>>,
    node_to_p2p_rx: Receiver<Vec<u8>>,
}

impl AVSp2p {
    pub fn new(node_tx: Sender<Vec<u8>>, node_to_p2p_rx: Receiver<Vec<u8>>) -> Self {
        AVSp2p {
            node_tx,
            node_to_p2p_rx,
        }
    }

    // Consumes self and fires up threads
    pub async fn start(self, config: P2PNetworkConfig) {
        info!("Starting P2P network");

        let mut p2p = P2PNetwork::new(&config, self.node_tx.clone(), self.node_to_p2p_rx).await;

        task::spawn(async move {
            p2p.run(&config).await;
        });
    }
}
