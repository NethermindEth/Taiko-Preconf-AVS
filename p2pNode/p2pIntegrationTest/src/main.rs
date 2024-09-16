use p2p_network::generate_secp256k1;
use p2p_network::network::{P2PNetwork, P2PNetworkConfig};
use rand::Rng;
use std::fs::File;
use std::io::Write;
use std::io::{self, Read};
use std::path::Path;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task;
use tracing::{info, warn};

fn generate_1mb_vec(count: u32) -> Vec<u8> {
    let count: u8 = if count > 255 { 0u8 } else { count as u8 };
    let mut vec = vec![0u8; 1_048_576]; // 1 MB of zeros initially
    vec[0] = count;
    rand::thread_rng().fill(&mut vec[1..]); // Fill with random data
    vec
}

const BOOT_NODE_PATH: &str = "/shared/enr.txt";

fn read_boot_node() -> Result<String, io::Error> {
    info!("Reading boot node from {}", BOOT_NODE_PATH);
    let mut file = File::open(BOOT_NODE_PATH)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    info!("Boot node: {}", contents);
    Ok(contents)
}

fn write_boot_node(enr: &str) -> Result<(), io::Error> {
    info!("Writing boot node to {} end {}", BOOT_NODE_PATH, enr);
    let mut file = File::create(BOOT_NODE_PATH)?;
    file.write_all(enr.as_bytes())?;
    Ok(())
}

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

    // Load boot node from shared directory
    let path = Path::new(BOOT_NODE_PATH);
    let boot_nodes: Option<Vec<String>> = if path.exists() {
        let bootnode = read_boot_node().unwrap();
        Some(vec![bootnode])
    } else {
        None
    };

    let config = P2PNetworkConfig {
        local_key: generate_secp256k1(),
        listen_addr: "/ip4/0.0.0.0/tcp/9000".parse().unwrap(),
        ipv4,
        udpv4: 9000,
        tcpv4: 9000,
        boot_nodes,
    };
    let (node_to_p2p_tx, node_to_p2p_rx) = mpsc::channel(10);
    let (node_tx, mut node_rx) = mpsc::channel(10);
    let mut p2p = P2PNetwork::new(&config, node_tx.clone(), node_to_p2p_rx).await;

    // Save boot node if it is not specified in shared directory
    if config.boot_nodes.is_none() {
        warn!("Boot node not specified yet. Saving to {}", BOOT_NODE_PATH);
        write_boot_node(&p2p.get_local_enr()).unwrap();
    }

    task::spawn(async move {
        p2p.run(&config).await;
    });

    // Load SEND from env
    let send_prefix = std::env::var("SEND_PREFIX").unwrap();
    info!("SEND PREFIX: {send_prefix}");
    let mut send_count = 1;
    let mut send_interval = tokio::time::interval(Duration::from_secs(40));
    // Run
    loop {
        tokio::select! {
            _ = send_interval.tick() => {
                send_count += 1;
                let data = generate_1mb_vec(send_count);
                info!("SEND Message: {}", send_count);

                node_to_p2p_tx
                    .send(data)
                    .await
                    .unwrap();
            }
            Some(message) = node_rx.recv() => {
                info!("Node received message: {} size {}", message[0], message.len());
            }
        }
    }
}
