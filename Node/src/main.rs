mod ethereum_l1;
mod node;
mod shared;
mod taiko;
mod utils;

use anyhow::Error;
use std::sync::Arc;
use tokio::signal::unix::{signal, SignalKind};
use tokio_util::sync::CancellationToken;
use tracing::info;

#[tokio::main]
async fn main() -> Result<(), Error> {
    init_logging();

    info!("🚀 Starting Whitelist Node v{}", env!("CARGO_PKG_VERSION"));

    let config = utils::config::Config::read_env_variables();
    let cancel_token = CancellationToken::new();

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
            config.taiko_anchor_address,
        )
        .await?,
    );

    let max_anchor_height_offset = ethereum_l1
        .execution_layer
        .get_pacaya_config_max_anchor_height_offset();
    if config.max_anchor_height_offset_reduction >= max_anchor_height_offset {
        panic!(
            "max_anchor_height_offset_reduction {} is greater than max_anchor_height_offset from pacaya config {}",
            config.max_anchor_height_offset_reduction, max_anchor_height_offset
        );
    }
    let max_blocks_per_batch = ethereum_l1
        .execution_layer
        .get_pacaya_config_max_blocks_per_batch();
    if config.max_blocks_per_batch_reduction >= max_blocks_per_batch {
        panic!(
            "max_blocks_per_batch {} is greater than max_blocks_per_batch from pacaya config {}",
            config.max_blocks_per_batch_reduction, max_blocks_per_batch
        );
    }

    let node = node::Node::new(
        cancel_token.clone(),
        taiko.clone(),
        ethereum_l1.clone(),
        config.preconf_heartbeat_ms,
        config.handover_window_slots,
        config.handover_start_buffer_ms,
        config.l1_height_lag,
        node::batch_manager::BatchBuilderConfig {
            max_bytes_size_of_batch: config.max_bytes_size_of_batch,
            max_blocks_per_batch: max_blocks_per_batch - config.max_blocks_per_batch_reduction,
            l1_slot_duration_sec: config.l1_slot_duration_sec,
            max_time_shift_between_blocks_sec: config.max_time_shift_between_blocks_sec,
            max_anchor_height_offset: max_anchor_height_offset
                - config.max_anchor_height_offset_reduction,
        },
    )
    .await?;
    node.entrypoint();

    wait_for_the_termination(cancel_token, config.l1_slot_duration_sec).await;

    Ok(())
}

async fn wait_for_the_termination(cancel_token: CancellationToken, shutdown_delay_secs: u64) {
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to set up SIGTERM handler");
    tokio::select! {
        _ = sigterm.recv() => {
            info!("Received SIGTERM, shutting down...");
            cancel_token.cancel();
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl+C, shutting down...");
            cancel_token.cancel();
        }
    }

    // Give tasks a little time to finish
    info!("Waiting for {}s", shutdown_delay_secs);
    tokio::time::sleep(tokio::time::Duration::from_secs(shutdown_delay_secs)).await;
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
