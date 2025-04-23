use discv5::{enr, enr::CombinedKey, ConfigBuilder, Discv5, Event, ListenConfig};
use jsonrpc_core::{IoHandler, Params, Result, Value};
use jsonrpc_http_server::ServerBuilder;
use std::{net::Ipv4Addr, sync::Arc};
use tracing::info;

// This is the handler for the JSON-RPC methods.
fn create_rpc_handler(enr: Arc<String>) -> IoHandler {
    let mut io = IoHandler::new();

    io.add_method("p2p_getENR", move |_params: Params| {
        let s = enr.to_string();
        async move { Ok(Value::String(s)) }
    });

    // Add a health check method
    io.add_method("health", |_params: Params| {
        async move {
            // Return a simple response indicating the service is healthy
            Ok(Value::String("Ok".to_string()))
        }
    });

    io
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

    std::thread::spawn(move || {
        let io_handler = create_rpc_handler(enr_base64);

        let addr = "0.0.0.0:9001".to_string();

        let server = ServerBuilder::new(io_handler)
            //.cors(Method::POST)
            .start_http(&addr.parse().unwrap())
            .expect("Unable to start RPC server");

        info!("RPC server started on port 9001");
        server.wait();
    });

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
