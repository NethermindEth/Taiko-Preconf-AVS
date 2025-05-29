use anyhow::Result;
use jsonrpsee::{
    core::client::ClientT,
    http_client::{HttpClient, HttpClientBuilder},
};
use p2p_network::generate_secp256k1;
use p2p_network::network::{P2PNetwork, P2PNetworkConfig};
use rand::Rng;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::task;
use tracing::{info, warn};

fn generate_1mb_vec(count: u32) -> Vec<u8> {
    let count: u8 = if count > 255 { 0u8 } else { count as u8 };
    let mut vec = vec![0u8; 1_048_576]; // 1 MB of zeros initially
    vec[0] = count;
    rand::rng().fill(&mut vec[1..]); // Fill with random data
    vec
}

async fn get_boot_node_enr(boot_node_ip: String) -> Result<String> {
    // Define the RPC endpoint
    let boot_node_url = format!("http://{}:9001", boot_node_ip);

    let client: HttpClient = HttpClientBuilder::default().build(&boot_node_url)?;

    let response: String = client
        .request("p2p_getENR", jsonrpsee::rpc_params![])
        .await?;
    info!("Response: {}", response);

    // Remove surrounding quotes
    Ok(response.to_string().trim_matches('"').to_string())
}

#[tokio::main]
async fn main() -> Result<()> {
    // Setup tracing
    let filter_layer = tracing_subscriber::EnvFilter::try_from_default_env()
        .or_else(|_| tracing_subscriber::EnvFilter::try_new("debug"))
        .unwrap();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter_layer)
        .try_init();

    let address = std::env::var("ADDRESS").unwrap_or_else(|_| "0.0.0.0".to_string());
    let ipv4 = address.parse()?;
    info!("ADDRESS: {address:?}");

    // get boot node ip
    let boot_node_ip = std::env::var("BOOT_NODE_IP")?;

    // Get boot node by JSON-RPC
    let bootnode_enr = get_boot_node_enr(boot_node_ip).await?;
    info!("Boot node ENR: {bootnode_enr}");
    let boot_nodes = Some(vec![bootnode_enr]);

    let config = P2PNetworkConfig {
        local_key: generate_secp256k1(),
        listen_addr: "/ip4/0.0.0.0/tcp/9000".parse()?,
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
        warn!("Boot node not specified!");
    }

    task::spawn(async move {
        p2p.run(&config).await;
    });

    let send_prefix = std::env::var("SEND_PREFIX")?;
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
                    .await?;
            }
            Some(message) = node_rx.recv() => {
                info!("Node received message: {} size {}", message[0], message.len());
            }
        }
    }
}
