mod ethereum_l1;
mod mev_boost;
mod node;
mod p2p_network;
mod taiko;
mod utils;

use anyhow::Error;
use tokio::sync::mpsc;

const MESSAGE_QUEUE_SIZE: usize = 100;

#[tokio::main]
async fn main() -> Result<(), Error> {
    init_logging();

    let (avs_p2p_tx, avs_p2p_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);
    let (node_tx, node_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);
    let p2p = p2p_network::AVSp2p::new(node_tx.clone(), avs_p2p_rx);
    p2p.start();
    let ethereum_l1 = ethereum_l1::EthereumL1::new("http://localhost:8545", "private_key")?;
    let node = node::Node::new(node_rx, avs_p2p_tx, ethereum_l1);
    node.entrypoint().await?;
    Ok(())
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();
}
