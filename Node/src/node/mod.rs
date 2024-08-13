use crate::{
    ethereum_l1::{
        slot_clock::{Epoch, Slot},
        EthereumL1,
    },
    mev_boost::MevBoost,
    taiko::Taiko,
    utils::node_message::NodeMessage,
};
use anyhow::{anyhow as any_err, Error};
use beacon_api_client::ProposerDuty;
use std::sync::Arc;
use tokio::sync::mpsc::{Receiver, Sender};

pub mod block_proposed_receiver;

pub struct Node {
    taiko: Arc<Taiko>,
    node_rx: Option<Receiver<NodeMessage>>,
    avs_p2p_tx: Sender<String>,
    gas_used: u64,
    ethereum_l1: EthereumL1,
    _mev_boost: MevBoost, // temporary unused
    epoch: Epoch,
    lookahead: Vec<ProposerDuty>,
    l2_slot_duration_sec: u64,
    validator_pubkey: String,
    current_slot_to_preconf: Option<Slot>,
    next_slot_to_preconf: Option<Slot>,
}

impl Node {
    pub async fn new(
        node_rx: Receiver<NodeMessage>,
        avs_p2p_tx: Sender<String>,
        taiko: Arc<Taiko>,
        ethereum_l1: EthereumL1,
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
        if let Some(node_rx) = self.node_rx.take() {
            tokio::spawn(async move {
                Self::handle_incoming_messages(node_rx).await;
            });
        } else {
            tracing::error!("node_rx has already been moved");
        }
    }

    async fn handle_incoming_messages(mut node_rx: Receiver<NodeMessage>) {
        loop {
            tokio::select! {
                Some(message) = node_rx.recv() => {
                    match message {
                        NodeMessage::BlockProposed(block_proposed) => {
                            tracing::debug!("Node received block proposed event: {:?}", block_proposed);
                        }
                        NodeMessage::P2P(message) => {
                            tracing::debug!("Node received P2P message: {:?}", message);
                        }
                    }
                },
            }
        }
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

        self.commit_to_the_tx_lists();
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

        Ok(())
    }

    fn check_for_the_slot_to_preconf(&self, lookahead: &[ProposerDuty]) -> Option<Slot> {
        lookahead
            .iter()
            .find(|duty| duty.public_key.to_string() == self.validator_pubkey)
            .map(|duty| duty.slot)
    }

    fn commit_to_the_tx_lists(&self) {
        //TODO: implement
    }

    async fn send_preconfirmations_to_the_avs_p2p(&self) -> Result<(), Error> {
        self.avs_p2p_tx
            .send("Hello from node!".to_string())
            .await
            .map_err(|e| any_err!("Failed to send message to avs_p2p_tx: {}", e))
    }
}
