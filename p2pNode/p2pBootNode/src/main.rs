use discv5::{enr, enr::CombinedKey, ConfigBuilder, Discv5, Event, ListenConfig};
use std::fs::File;
use std::io::{self, Write};
use std::net::Ipv4Addr;
use tracing::{error, info};

const BOOT_NODE_PATH: &str = "/shared/enr.txt";

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

    if let Err(e) = write_boot_node(&discv5.local_enr().to_base64()) {
        error!("Failed to write boot node to file: {}", e);
    }

    // start the discv5 service
    discv5.start().await.unwrap();
    info!("Server started");

    // get an event stream
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
