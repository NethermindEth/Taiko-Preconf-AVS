mod crypto;
mod ethereum_l1;
mod funds_monitor;
mod metrics;
mod node;
mod reorg_detector;
mod shared;
mod taiko;
mod utils;

use anyhow::Error;
use metrics::Metrics;
use std::sync::Arc;
use tokio::{
    signal::unix::{SignalKind, signal},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

#[cfg(feature = "test-gas")]
mod test_gas;
#[cfg(feature = "test-gas")]
use clap::Parser;
#[cfg(feature = "test-gas")]
use test_gas::test_gas_params;

#[cfg(feature = "test-gas")]
#[derive(Parser, Debug)]
struct Args {
    #[arg(long = "test-gas", value_name = "BLOCK_COUNT")]
    test_gas: Option<u32>,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    init_logging();

    info!("🚀 Starting Whitelist Node v{}", env!("CARGO_PKG_VERSION"));

    let config = utils::config::Config::read_env_variables();
    let cancel_token = CancellationToken::new();

    let metrics = Arc::new(Metrics::new());

    // Set up panic hook to cancel token on panic
    let panic_cancel_token = cancel_token.clone();
    std::panic::set_hook(Box::new(move |panic_info| {
        error!("Panic occurred: {:?}", panic_info);
        panic_cancel_token.cancel();
        info!("Cancellation token triggered, initiating shutdown...");
    }));

    let (transaction_error_sender, transaction_error_receiver) = mpsc::channel(100);
    let ethereum_l1 = ethereum_l1::EthereumL1::new(
        ethereum_l1::config::EthereumL1Config {
            execution_ws_rpc_url: config.l1_ws_rpc_url.clone(),
            avs_node_ecdsa_private_key: config.avs_node_ecdsa_private_key.clone(),
            contract_addresses: config.contract_addresses.clone(),
            consensus_rpc_url: config.l1_beacon_url,
            slot_duration_sec: config.l1_slot_duration_sec,
            slots_per_epoch: config.l1_slots_per_epoch,
            preconf_heartbeat_ms: config.preconf_heartbeat_ms,
            min_priority_fee_per_gas_wei: config.min_priority_fee_per_gas_wei,
            tx_fees_increase_percentage: config.tx_fees_increase_percentage,
            max_attempts_to_send_tx: config.max_attempts_to_send_tx,
            max_attempts_to_wait_tx: config.max_attempts_to_wait_tx,
            delay_between_tx_attempts_sec: config.delay_between_tx_attempts_sec,
        },
        transaction_error_sender,
        metrics.clone(),
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to create EthereumL1: {}", e))?;

    let ethereum_l1 = Arc::new(ethereum_l1);

    #[cfg(feature = "test-gas")]
    let args = Args::parse();
    #[cfg(feature = "test-gas")]
    if let Some(gas) = args.test_gas {
        info!("Test gas block count: {}", gas);
        test_gas_params(
            ethereum_l1.clone(),
            gas,
            config.l1_height_lag,
            config.max_bytes_size_of_batch,
            transaction_error_receiver,
        )
        .await?;
        return Ok(());
    } else {
        tracing::error!("No test gas block count provided.");
    }

    let jwt_secret_bytes = utils::file_operations::read_jwt_secret(&config.jwt_secret_file_path)?;
    let taiko = Arc::new(
        taiko::Taiko::new(
            ethereum_l1.clone(),
            metrics.clone(),
            taiko::config::TaikoConfig::new(
                config.taiko_geth_ws_rpc_url.clone(),
                config.taiko_geth_auth_rpc_url,
                config.taiko_driver_url,
                jwt_secret_bytes,
                config.taiko_anchor_address,
                config.taiko_bridge_address,
                config.max_bytes_per_tx_list,
                config.min_bytes_per_tx_list,
                config.throttling_factor,
                config.rpc_l2_execution_layer_timeout,
                config.rpc_driver_preconf_timeout,
                config.rpc_driver_status_timeout,
                config.avs_node_ecdsa_private_key,
            )?,
        )
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create Taiko: {}", e))?,
    );

    let max_anchor_height_offset = ethereum_l1
        .execution_layer
        .get_config_max_anchor_height_offset();
    if config.max_anchor_height_offset_reduction >= max_anchor_height_offset {
        panic!(
            "max_anchor_height_offset_reduction {} is greater than max_anchor_height_offset from pacaya config {}",
            config.max_anchor_height_offset_reduction, max_anchor_height_offset
        );
    }

    let l1_max_blocks_per_batch = ethereum_l1
        .execution_layer
        .get_config_max_blocks_per_batch();

    if config.max_blocks_per_batch > l1_max_blocks_per_batch {
        panic!(
            "max_blocks_per_batch ({}) exceeds limit from Pacaya config ({})",
            config.max_blocks_per_batch, l1_max_blocks_per_batch
        );
    }

    let max_blocks_per_batch = if config.max_blocks_per_batch == 0 {
        l1_max_blocks_per_batch
    } else {
        config.max_blocks_per_batch
    };

    let reorg_detector = Arc::new(
        reorg_detector::ReorgDetector::new(
            config.l1_ws_rpc_url,
            config.taiko_geth_ws_rpc_url,
            config.contract_addresses.taiko_inbox,
            cancel_token.clone(),
        )
        .map_err(|e| anyhow::anyhow!("Failed to create ReorgDetector: {}", e))?,
    );
    reorg_detector
        .start()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start ReorgDetector: {}", e))?;

    let node = node::Node::new(
        cancel_token.clone(),
        taiko.clone(),
        ethereum_l1.clone(),
        reorg_detector.clone(),
        config.preconf_heartbeat_ms,
        config.handover_window_slots,
        config.handover_start_buffer_ms,
        config.l1_height_lag,
        node::batch_manager::BatchBuilderConfig {
            max_bytes_size_of_batch: config.max_bytes_size_of_batch,
            max_blocks_per_batch,
            l1_slot_duration_sec: config.l1_slot_duration_sec,
            max_time_shift_between_blocks_sec: config.max_time_shift_between_blocks_sec,
            max_anchor_height_offset: max_anchor_height_offset
                - config.max_anchor_height_offset_reduction,
            default_coinbase: ethereum_l1.execution_layer.get_preconfer_alloy_address(),
        },
        config.simulate_not_submitting_at_the_end_of_epoch,
        transaction_error_receiver,
        metrics.clone(),
    )
    .await
    .map_err(|e| anyhow::anyhow!("Failed to create Node: {}", e))?;

    node.entrypoint()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to start Node: {}", e))?;

    let funds_monitor = funds_monitor::FundsMonitor::new(
        ethereum_l1.clone(),
        taiko.clone(),
        metrics.clone(),
        config.threshold_eth,
        config.threshold_taiko,
        config.amount_to_bridge_from_l2_to_l1,
        cancel_token.clone(),
    );
    funds_monitor.run();

    metrics::server::serve_metrics(metrics.clone(), cancel_token.clone());

    wait_for_the_termination(cancel_token, config.l1_slot_duration_sec).await;

    Ok(())
}

async fn wait_for_the_termination(cancel_token: CancellationToken, shutdown_delay_secs: u64) {
    info!("Starting signal handler...");
    let mut sigterm = signal(SignalKind::terminate()).expect("Failed to set up SIGTERM handler");
    tokio::select! {
        _ = sigterm.recv() => {
            info!("Received SIGTERM, shutting down...");
            cancel_token.cancel();
            // Give tasks a little time to finish
            info!("Waiting for {}s", shutdown_delay_secs);
            tokio::time::sleep(tokio::time::Duration::from_secs(shutdown_delay_secs)).await;
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received Ctrl+C, shutting down...");
            cancel_token.cancel();
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
        _ = cancel_token.cancelled() => {
            info!("Shutdown signal received, exiting avs node...");
        }
    }
}

fn init_logging() {
    use tracing_subscriber::{EnvFilter, filter::FilterFn, fmt, prelude::*};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new("debug")
            .add_directive(
                "reqwest=info"
                    .parse()
                    .expect("assert: can parse env filter directive"),
            )
            .add_directive(
                "hyper=info"
                    .parse()
                    .expect("assert: can parse env filter directive"),
            )
            .add_directive(
                "alloy_transport=info"
                    .parse()
                    .expect("assert: can parse env filter directive"),
            )
            .add_directive(
                "alloy_rpc_client=info"
                    .parse()
                    .expect("assert: can parse env filter directive"),
            )
            .add_directive(
                "p2p_network=info"
                    .parse()
                    .expect("assert: can parse env filter directive"),
            )
            .add_directive(
                "libp2p_gossipsub=info"
                    .parse()
                    .expect("assert: can parse env filter directive"),
            )
            .add_directive(
                "discv5=info"
                    .parse()
                    .expect("assert: can parse env filter directive"),
            )
            .add_directive(
                "netlink_proto=info"
                    .parse()
                    .expect("assert: can parse env filter directive"),
            )
    });

    // Create a custom formatter for heartbeat logs
    let heartbeat_format = fmt::format()
        .with_timer(fmt::time::time())
        .with_target(false)
        .with_level(false)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false);

    // Create a standard formatter for all other logs
    let standard_format = fmt::format()
        .with_timer(fmt::time::time())
        .with_target(true)
        .with_level(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .with_file(false)
        .with_line_number(false);

    // Create a layered subscriber that uses different formatters based on the target
    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::Layer::default()
                .with_writer(std::io::stdout)
                .event_format(standard_format)
                .with_filter(FilterFn::new(|metadata: &tracing::Metadata<'_>| {
                    !metadata.target().contains("heartbeat")
                })),
        )
        .with(
            fmt::Layer::default()
                .with_writer(std::io::stdout)
                .event_format(heartbeat_format)
                .with_filter(FilterFn::new(|metadata: &tracing::Metadata<'_>| {
                    metadata.target().contains("heartbeat")
                })),
        );

    subscriber.init();
}
