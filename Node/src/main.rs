mod ethereum_l1;
mod node;
mod taiko;
mod utils;

use anyhow::Error;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Error> {
    init_logging();

    tracing::info!("🚀 Starting Whitelist Node v{}", env!("CARGO_PKG_VERSION"));

    let config = utils::config::Config::read_env_variables();

    let ethereum_l1 = ethereum_l1::EthereumL1::new(
        &config.l1_ws_rpc_url,
        &config.avs_node_ecdsa_private_key,
        &config.contract_addresses,
        &config.l1_beacon_url,
        config.l1_slot_duration_sec,
        config.l1_slots_per_epoch,
        config.preconf_heartbeat_ms,
    )
    .await?;

    let ethereum_l1 = Arc::new(ethereum_l1);

    let jwt_secret_bytes = utils::file_operations::read_jwt_secret(&config.jwt_secret_file_path)?;
    let taiko = Arc::new(
        taiko::Taiko::new(
            &config.taiko_geth_ws_rpc_url,
            &config.taiko_geth_auth_rpc_url,
            &config.taiko_driver_url,
            config.rpc_client_timeout,
            &jwt_secret_bytes,
            ethereum_l1.execution_layer.get_preconfer_address(),
            ethereum_l1.clone(),
            config.taiko_l2_address,
        )
        .await?,
    );

    let node = node::Node::new(
        taiko.clone(),
        ethereum_l1.clone(),
        config.preconf_heartbeat_ms,
        config.handover_window_slots,
        config.handover_start_buffer_ms,
    )
    .await?;
    node.entrypoint().await?;

    Ok(())
}

fn init_logging() {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("debug")
            .add_directive("reqwest=info".parse().unwrap())
            .add_directive("hyper=info".parse().unwrap())
            .add_directive("alloy_transport=info".parse().unwrap())
            .add_directive("alloy_rpc_client=info".parse().unwrap())
            .add_directive("p2p_network=info".parse().unwrap())
            .add_directive("libp2p_gossipsub=info".parse().unwrap())
            .add_directive("discv5=info".parse().unwrap())
            .add_directive("netlink_proto=info".parse().unwrap())
    });

    fmt().with_env_filter(filter).init();
}
