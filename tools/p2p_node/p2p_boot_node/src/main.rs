use anyhow::Result;
use discv5::{ConfigBuilder, Discv5, Event, ListenConfig, enr, enr::CombinedKey};
use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};
use tracing::info;

use jsonrpsee::server::{RpcModule, ServerBuilder};

fn create_rpc_handler(enr: Arc<String>) -> Result<RpcModule<()>> {
    let mut module = RpcModule::new(());

    module.register_method("p2p_getENR", move |_, _, _| enr.to_string())?;

    module.register_method("health", |_, _, _| {
        // Return a simple response indicating the service is healthy
        "Ok".to_string()
    })?;
    Ok(module)
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

    // if there is an address specified use it
    let address = std::env::args()
        .nth(1)
        .map(|addr| addr.parse::<Ipv4Addr>().unwrap());
    info!("IP address: {address:?}");

    // if there is a port specified use it
    let port = {
        if let Some(udp_port) = std::env::args().nth(2) {
            udp_port.parse().unwrap()
        } else {
            9000
        }
    };
    info!("UDP port: {port}");

    // listening address and port
    let listen_config = ListenConfig::Ipv4 {
        ip: Ipv4Addr::UNSPECIFIED,
        port: 9000,
    };

    let enr_key = CombinedKey::generate_secp256k1();

    // construct a local ENR
    let enr = {
        let mut builder = enr::Enr::builder();
        // if an IP was specified, use it
        if let Some(external_address) = address {
            builder.ip4(external_address);
        }
        // if a port was specified, use it
        if std::env::args().nth(2).is_some() {
            builder.udp4(port);
        }
        builder.build(&enr_key).unwrap()
    };

    // if the ENR is useful print it
    info!("Node Id: {}", enr.node_id());
    if enr.udp4_socket().is_some() {
        info!("Base64 ENR: {}", enr.to_base64());
        info!(
            "IP: {}, UDP_PORT:{}",
            enr.ip4().unwrap(),
            enr.udp4().unwrap()
        );
    } else {
        info!("ENR is not printed as no IP:PORT was specified");
    }

    // default configuration
    let config = ConfigBuilder::new(listen_config).build();

    // construct the discv5 server
    let mut discv5: Discv5 = Discv5::new(enr, enr_key, config).unwrap();

    // save base64 ENR
    let enr_base64 = Arc::new(discv5.local_enr().to_base64());

    // Start the JSON-RPC server in a separate tokio task
    let rpc_enr_base64 = Arc::clone(&enr_base64); // Clone for the RPC thread
    let module = create_rpc_handler(rpc_enr_base64)?;

    let addr: SocketAddr = "0.0.0.0:9001".parse().unwrap();
    info!("RPC server to be started on {addr}");
    let server = ServerBuilder::default()
        .build(addr)
        .await
        .expect("Unable to start RPC server");
    let handle = server.start(module);
    // we don't care about doing shutdown
    tokio::spawn(handle.stopped());

    // Start the discv5 service
    discv5.start().await.unwrap();
    info!("Discv5 server started");

    // Start the event loop for discv5
    let mut event_stream = discv5.event_stream().await.unwrap();

    loop {
        match event_stream.recv().await {
            Some(Event::SocketUpdated(addr)) => {
                info!("Nodes ENR socket address has been updated to: {addr:?}");
            }
            Some(Event::Discovered(enr)) => {
                info!("A peer has been discovered: {}", enr.node_id());
            }
            _ => {}
        }
    }
}
