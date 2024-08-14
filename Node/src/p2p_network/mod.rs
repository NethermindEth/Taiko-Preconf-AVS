use p2p_network::generate_secp256k1;
use p2p_network::network::{P2PNetwork, P2PNetworkConfig};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task;
use tracing::info;

pub struct AVSp2p {
    node_tx: Sender<Vec<u8>>,
    avs_p2p_rx: Receiver<Vec<u8>>,
}

impl AVSp2p {
    pub fn new(node_tx: Sender<Vec<u8>>, avs_p2p_rx: Receiver<Vec<u8>>) -> Self {
        AVSp2p {
            node_tx,
            avs_p2p_rx,
        }
    }

    // Consumes self and fires up threads
    pub async fn start(self) {
        info!("Starting P2P network");

        // Load ADDRESS from env
        // TODO Move to a config file
        let address = std::env::var("ADDRESS").unwrap_or_else(|_| "0.0.0.0".to_string());
        let ipv4 = address.parse().unwrap();
        info!("Node ipv4 address: {address:?}");

        let config = P2PNetworkConfig {
            local_key: generate_secp256k1(),
            listen_addr: "/ip4/0.0.0.0/tcp/9000".parse().unwrap(),
            ipv4,
            udpv4: 9000,
            tcpv4: 9000,
        };

        let mut p2p = P2PNetwork::new(&config, self.node_tx.clone(), self.avs_p2p_rx).await;

        task::spawn(async move {
            p2p.run(&config).await;
        });
    }
}
