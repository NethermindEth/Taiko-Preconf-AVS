mod bls;
mod ethereum_l1;
mod mev_boost;
mod node;
mod p2p_network;
mod taiko;
mod utils;

use anyhow::Error;
use node::
    block_proposed_receiver::BlockProposedEventReceiver;
use std::sync::Arc;
use tokio::sync::mpsc;

const MESSAGE_QUEUE_SIZE: usize = 100;

#[tokio::main]
async fn main() -> Result<(), Error> {
    init_logging();

    tracing::info!("🚀 Starting Whitelist Node v{}", env!("CARGO_PKG_VERSION"));

    let config = utils::config::Config::read_env_variables();

    let bls_service = Arc::new(bls::BLSService::new(&config.validator_bls_privkey)?);

    let ethereum_l1 = ethereum_l1::EthereumL1::new(
        &config.l1_ws_rpc_url,
        &config.avs_node_ecdsa_private_key,
        &config.contract_addresses,
        &config.l1_beacon_url,
        config.l1_slot_duration_sec,
        config.l1_slots_per_epoch,
        config.l2_slot_duration_sec,
    )
    .await?;

    let (node_to_p2p_tx, node_to_p2p_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);
    let (p2p_to_node_tx, p2p_to_node_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);
    let (block_proposed_tx, block_proposed_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);
    if config.enable_p2p {
        let p2p = p2p_network::AVSp2p::new(p2p_to_node_tx.clone(), node_to_p2p_rx);
        p2p.start(config.p2p_network_config).await;
    }
    let taiko = Arc::new(taiko::Taiko::new(
        &config.taiko_proposer_url,
        &config.taiko_driver_url,
        config.taiko_chain_id,
    ));

    let ethereum_l1 = Arc::new(ethereum_l1);
    let mev_boost = mev_boost::MevBoost::new(
        &config.mev_boost_url,
        ethereum_l1
            .consensus_layer
            .get_genesis_details()
            .await?
            .genesis_fork_version,
    );

    let block_proposed_event_checker =
        BlockProposedEventReceiver::new(ethereum_l1.clone(), block_proposed_tx);
    BlockProposedEventReceiver::start(block_proposed_event_checker);

    let node = node::Node::new(
        block_proposed_rx,
        node_to_p2p_tx,
        p2p_to_node_rx,
        taiko.clone(),
        ethereum_l1.clone(),
        mev_boost,
        config.l2_slot_duration_sec,
        bls_service,
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
