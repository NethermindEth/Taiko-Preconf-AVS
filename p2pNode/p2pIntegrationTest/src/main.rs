use p2p_network::generate_secp256k1;
use p2p_network::network::{P2PNetwork, P2PNetworkConfig};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Setup tracing
    let filter_layer = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| tracing_subscriber::EnvFilter::try_new("debug"))
        .unwrap();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter_layer)
        .try_init();
    // Load ADDRESS from env
    let address = std::env::var("ADDRESS").unwrap_or_else(|_| "0.0.0.0".to_string());
    let ipv4 = address.parse().unwrap();
    info!("ADDRESS: {address:?}");

    let config = P2PNetworkConfig {
        local_key: generate_secp256k1(),
        listen_addr: "/ip4/0.0.0.0/tcp/9000".parse().unwrap(),
        ipv4,
        udpv4: 9000,
        tcpv4: 9000,
    };
    let (avs_p2p_tx, avs_p2p_rx) = mpsc::channel(10);
    let (node_tx, mut node_rx) = mpsc::channel(10);
    let mut p2p = P2PNetwork::new(&config, node_tx.clone(), avs_p2p_rx).await;

    task::spawn(async move {
        p2p.run(&config).await;
    });

    // Load SEND from env
    let send_prefix = std::env::var("SEND_PREFIX").unwrap();
    info!("SEND PREFIX: {send_prefix}");
    let mut send_count = 1;
    let mut send_interval = tokio::time::interval(Duration::from_secs(20));
    // Run
    loop {
        tokio::select! {
            _ = send_interval.tick() => {
                send_count += 1;
                let data = format!("{send_prefix}-{send_count}");
                info!("SEND Message: {:#?}", &data);

                avs_p2p_tx
                    .send(data.as_bytes().to_vec())
                    .await
                    .unwrap();
            }
            Some(message) = node_rx.recv() => {
                let string = String::from_utf8(message).expect("Invalid UTF-8");
                info!("Node received message: {}", string);
            }
        }
    }
}
