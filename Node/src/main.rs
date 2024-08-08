mod ethereum_l1;
mod mev_boost;
mod node;
mod p2p_network;
mod taiko;
mod utils;

use anyhow::Error;
use node::block_proposed_receiver::BlockProposedEventReceiver;
use std::sync::Arc;
use tokio::sync::mpsc;
use utils::node_message::NodeMessage;

const MESSAGE_QUEUE_SIZE: usize = 100;

#[tokio::main]
async fn main() -> Result<(), Error> {
    init_logging();
    let config = utils::config::Config::read_env_variables();

    let (avs_p2p_tx, avs_p2p_rx) = mpsc::channel(MESSAGE_QUEUE_SIZE);
    let (node_tx, node_rx) = mpsc::channel::<NodeMessage>(MESSAGE_QUEUE_SIZE);
    let p2p = p2p_network::AVSp2p::new(node_tx.clone(), avs_p2p_rx);
    p2p.start();
    let taiko = Arc::new(taiko::Taiko::new(
        &config.taiko_proposer_url,
        &config.taiko_driver_url,
        config.block_proposed_receiver_timeout_sec,
    ));
    let ethereum_l1 = ethereum_l1::EthereumL1::new(
        &config.mev_boost_url,
        &config.ethereum_private_key,
        &config.taiko_preconfirming_address,
        &config.l1_beacon_url,
        config.l1_slot_duration_sec,
        config.l1_slots_per_epoch,
    )
    .await?;
    let mev_boost = mev_boost::MevBoost::new(&config.mev_boost_url);
    let block_proposed_event_checker =
        BlockProposedEventReceiver::new(taiko.clone(), node_tx.clone());
    BlockProposedEventReceiver::start(block_proposed_event_checker).await;
    let node = node::Node::new(
        node_rx,
        avs_p2p_tx,
        taiko,
        ethereum_l1,
        mev_boost,
        config.l2_slot_duration_sec,
        config.validator_pubkey,
    )
    .await?;
    node.entrypoint().await?;
    Ok(())
}

fn init_logging() {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();
}
