use crate::{
    ethereum_l1::{
        slot_clock::{Epoch, Slot},
        EthereumL1,
    },
    mev_boost::MevBoost,
    taiko::Taiko,
    utils::{block_proposed::BlockProposed, commit::L2TxListsCommit, node_message::NodeMessage},
};
use anyhow::{anyhow as any_err, Error};
use beacon_api_client::ProposerDuty;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{
    mpsc::{Receiver, Sender},
    Mutex,
};
use tracing::info;

mod block;
pub use block::Block;

pub mod block_proposed_receiver;

const OLDEST_BLOCK_DISTANCE: u64 = 256;

pub struct Node {
    taiko: Arc<Taiko>,
    node_rx: Option<Receiver<NodeMessage>>,
    avs_p2p_tx: Sender<String>,
    gas_used: u64,
    ethereum_l1: Arc<EthereumL1>,
    _mev_boost: MevBoost, // temporary unused
    epoch: Epoch,
    lookahead: Vec<ProposerDuty>,
    l2_slot_duration_sec: u64,
    validator_pubkey: String,
    current_slot_to_preconf: Option<Slot>,
    next_slot_to_preconf: Option<Slot>,
    preconfirmed_blocks: Arc<Mutex<HashMap<u64, Block>>>,
}

impl Node {
    pub async fn new(
        node_rx: Receiver<NodeMessage>,
        avs_p2p_tx: Sender<String>,
        taiko: Arc<Taiko>,
        ethereum_l1: Arc<EthereumL1>,
        mev_boost: MevBoost,
        l2_slot_duration_sec: u64,
        validator_pubkey: String,
    ) -> Result<Self, Error> {
        Ok(Self {
            taiko,
            node_rx: Some(node_rx),
            avs_p2p_tx,
            gas_used: 0,
            ethereum_l1,
            _mev_boost: mev_boost,
            epoch: Epoch::MAX, // it'll be updated in the first preconfirmation loop
            lookahead: vec![],
            l2_slot_duration_sec,
            validator_pubkey,
            current_slot_to_preconf: None,
            next_slot_to_preconf: None,
            preconfirmed_blocks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Consumes the Node and starts two loops:
    /// one for handling incoming messages and one for the block preconfirmation
    pub async fn entrypoint(mut self) -> Result<(), Error> {
        tracing::info!("Starting node");
        self.start_new_msg_receiver_thread();
        self.preconfirmation_loop().await;
        Ok(())
    }

    fn start_new_msg_receiver_thread(&mut self) {
        let preconfirmed_blocks = self.preconfirmed_blocks.clone();
        let ethereum_l1 = self.ethereum_l1.clone();
        if let Some(node_rx) = self.node_rx.take() {
            tokio::spawn(async move {
                Self::handle_incoming_messages(node_rx, preconfirmed_blocks, ethereum_l1).await;
            });
        } else {
            tracing::error!("node_rx has already been moved");
        }
    }

    async fn handle_incoming_messages(
        mut node_rx: Receiver<NodeMessage>,
        preconfirmed_blocks: Arc<Mutex<HashMap<u64, Block>>>,
        ethereum_l1: Arc<EthereumL1>,
    ) {
        loop {
            tokio::select! {
                Some(message) = node_rx.recv() => {
                    match message {
                        NodeMessage::BlockProposed(block_proposed) => {
                            tracing::debug!("Node received block proposed event: {:?}", block_proposed);
                            if let Err(e) = Self::check_preconfirmed_blocks_correctness(&preconfirmed_blocks, &block_proposed, ethereum_l1.clone()).await {
                                tracing::error!("Failed to check preconfirmed blocks correctness: {}", e);
                            }

                            if let Err(e) = Self::clean_old_blocks(&preconfirmed_blocks, block_proposed.block_id).await {
                                tracing::error!("Failed to clean old blocks: {}", e);
                            }
                        }
                        NodeMessage::P2P(message) => {
                            tracing::debug!("Node received P2P message: {:?}", message);
                        }
                    }
                },
            }
        }
    }

    async fn check_preconfirmed_blocks_correctness(
        preconfirmed_blocks: &Arc<Mutex<HashMap<u64, Block>>>,
        block_proposed: &BlockProposed,
        ethereum_l1: Arc<EthereumL1>,
    ) -> Result<(), Error> {
        let preconfirmed_blocks = preconfirmed_blocks.lock().await;
        if let Some(block) = preconfirmed_blocks.get(&block_proposed.block_id) {
            //TODO: verify the signature?

            if block.tx_list_hash != block_proposed.tx_list_hash {
                info!(
                    "Block tx_list_hash is not correct for block_id: {}. Calling proof of incorrect preconfirmation.",
                    block_proposed.block_id
                );
                ethereum_l1
                    .execution_layer
                    .prove_incorrect_preconfirmation(
                        block_proposed.block_id,
                        block.tx_list_hash,
                        block.signature,
                    )
                    .await?;
            }
        }
        Ok(())
    }

    async fn preconfirmation_loop(&mut self) {
        let mut interval =
            tokio::time::interval(std::time::Duration::from_secs(self.l2_slot_duration_sec));

        loop {
            interval.tick().await;

            if let Err(err) = self.main_block_preconfirmation_step().await {
                tracing::error!("Failed to execute main block preconfirmation step: {}", err);
            }
        }
    }

    async fn main_block_preconfirmation_step(&mut self) -> Result<(), Error> {
        let current_epoch = self.ethereum_l1.slot_clock.get_current_epoch()?;
        if current_epoch != self.epoch {
            tracing::debug!(
                "Current epoch changed from {} to {}",
                self.epoch,
                current_epoch
            );
            self.epoch = current_epoch;
            self.current_slot_to_preconf = self.next_slot_to_preconf;
            self.lookahead = self
                .ethereum_l1
                .consensus_layer
                .get_lookahead(self.epoch + 1)
                .await?;
            self.next_slot_to_preconf = self.check_for_the_slot_to_preconf(&self.lookahead);
        }

        if let Some(slot) = self.current_slot_to_preconf {
            if slot == self.ethereum_l1.slot_clock.get_current_slot()? {
                self.preconfirm_block().await?;
            }
        } else {
            tracing::debug!(
                "Not my slot to preconfirm: {}",
                self.ethereum_l1.slot_clock.get_current_slot()?
            );
        }

        Ok(())
    }

    async fn preconfirm_block(&mut self) -> Result<(), Error> {
        tracing::debug!(
            "Preconfirming for the slot: {:?}",
            self.current_slot_to_preconf
        );

        let pending_tx_lists = self.taiko.get_pending_l2_tx_lists().await?;
        if pending_tx_lists.tx_list_bytes.is_empty() {
            return Ok(());
        }

        let new_block_height = pending_tx_lists.parent_block_id + 1;
        let commit = L2TxListsCommit::new(&pending_tx_lists, new_block_height);

        self.send_preconfirmations_to_the_avs_p2p().await?;
        self.taiko
            .advance_head_to_new_l2_block(pending_tx_lists.tx_lists, self.gas_used)
            .await?;
        self.ethereum_l1
            .execution_layer
            .propose_new_block(
                pending_tx_lists.tx_list_bytes[0].clone(), //TODO: handle rest tx lists
                pending_tx_lists.parent_meta_hash,
                std::mem::take(&mut self.lookahead),
            )
            .await?;

        self.preconfirmed_blocks.lock().await.insert(
            new_block_height,
            Block {
                tx_list_hash: commit.hash()?,
                signature: [0; 96], // TODO: get the signature from the web3signer
            },
        );

        Ok(())
    }

    async fn clean_old_blocks(
        preconfirmed_blocks: &Arc<Mutex<HashMap<u64, Block>>>,
        current_block_height: u64,
    ) -> Result<(), Error> {
        let oldest_block_to_keep = current_block_height - OLDEST_BLOCK_DISTANCE;
        let mut preconfirmed_blocks = preconfirmed_blocks.lock().await;
        preconfirmed_blocks.retain(|block_height, _| block_height >= &oldest_block_to_keep);
        Ok(())
    }

    fn check_for_the_slot_to_preconf(&self, lookahead: &Vec<ProposerDuty>) -> Option<Slot> {
        lookahead
            .iter()
            .find(|duty| duty.public_key.to_string() == self.validator_pubkey)
            .map(|duty| duty.slot)
    }

    async fn send_preconfirmations_to_the_avs_p2p(&self) -> Result<(), Error> {
        self.avs_p2p_tx
            .send("Hello from node!".to_string())
            .await
            .map_err(|e| any_err!("Failed to send message to avs_p2p_tx: {}", e))
    }
}
