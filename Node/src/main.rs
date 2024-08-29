mod ethereum_l1;
mod mev_boost;
mod node;
mod p2p_network;
mod registration;
mod taiko;
mod utils;

use anyhow::Error;
use clap::Parser;
use node::{
    block_proposed_receiver::BlockProposedEventReceiver,
    lookahead_updated_receiver::LookaheadUpdatedEventReceiver,
};
use std::sync::Arc;
use tokio::sync::mpsc;

const MESSAGE_QUEUE_SIZE: usize = 100;

#[derive(Parser)]
struct Cli {
    #[clap(long, help = "Start registration as a preconfer")]
    register: bool,
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    init_logging();
    let args = Cli::parse();
    let config = utils::config::Config::read_env_variables();

    let ethereum_l1 = ethereum_l1::EthereumL1::new(
        &config.mev_boost_url,
        &config.avs_node_ecdsa_private_key,
        &config.contract_addresses,
        &config.l1_beacon_url,
        config.l1_slot_duration_sec,
        config.l1_slots_per_epoch,
        config.preconf_registry_expiry_sec,
    )
    .await?;

    if args.register {
        let registration = registration::Registration::new(ethereum_l1);
        registration.register().await?;
        return Ok(());
    }

    let (node_to_p2p_tx, node_to_p2p_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);
    let (p2p_to_node_tx, p2p_to_node_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);
    let (block_proposed_tx, block_proposed_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);
    let (lookahead_updated_tx, lookahead_updated_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);
    let p2p = p2p_network::AVSp2p::new(p2p_to_node_tx.clone(), node_to_p2p_rx);
    p2p.start(config.p2p_network_config).await;
    let taiko = Arc::new(taiko::Taiko::new(
        &config.taiko_proposer_url,
        &config.taiko_driver_url,
        config.block_proposed_receiver_timeout_sec,
        config.taiko_chain_id,
    ));

    let mev_boost = mev_boost::MevBoost::new(&config.mev_boost_url, config.validator_index);
    let ethereum_l1 = Arc::new(ethereum_l1);

    let node = node::Node::new(
        block_proposed_rx,
        node_to_p2p_tx,
        p2p_to_node_rx,
        lookahead_updated_rx,
        taiko.clone(),
        ethereum_l1.clone(),
        mev_boost,
        config.l2_slot_duration_sec,
    )
    .await?;
    node.entrypoint().await?;

    let block_proposed_event_checker = BlockProposedEventReceiver::new(taiko, block_proposed_tx);
    BlockProposedEventReceiver::start(block_proposed_event_checker);

    let lookahead_updated_event_checker =
        LookaheadUpdatedEventReceiver::new(ethereum_l1.clone(), lookahead_updated_tx);
    lookahead_updated_event_checker.start();

    Ok(())
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();
}
