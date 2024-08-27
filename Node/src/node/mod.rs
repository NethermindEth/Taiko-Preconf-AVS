use crate::{
    ethereum_l1::{execution_layer::PreconfTaskManager, slot_clock::Epoch, EthereumL1},
    mev_boost::MevBoost,
    taiko::{l2_tx_lists::RPCReplyL2TxLists, Taiko},
    utils::{
        block_proposed::BlockProposed, commit::L2TxListsCommit,
        preconfirmation_message::PreconfirmationMessage,
        preconfirmation_proof::PreconfirmationProof,
    },
};
use anyhow::{anyhow as any_err, Error};
use beacon_api_client::ProposerDuty;
use operator::{Operator, Status as OperatorStatus};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{
    mpsc::{Receiver, Sender},
    Mutex,
};
use tracing::info;

pub mod block_proposed_receiver;
mod operator;

const OLDEST_BLOCK_DISTANCE: u64 = 256;

pub struct Node {
    taiko: Arc<Taiko>,
    node_rx: Option<Receiver<BlockProposed>>,
    node_to_p2p_tx: Sender<Vec<u8>>,
    p2p_to_node_rx: Option<Receiver<Vec<u8>>>,
    gas_used: u64,
    ethereum_l1: Arc<EthereumL1>,
    _mev_boost: MevBoost, // temporary unused
    epoch: Epoch,
    lookahead: Vec<ProposerDuty>,
    l2_slot_duration_sec: u64,
    preconfirmed_blocks: Arc<Mutex<HashMap<u64, PreconfirmationProof>>>,
    operator: Operator,
}

impl Node {
    pub async fn new(
        node_rx: Receiver<BlockProposed>,
        node_to_p2p_tx: Sender<Vec<u8>>,
        p2p_to_node_rx: Receiver<Vec<u8>>,
        taiko: Arc<Taiko>,
        ethereum_l1: Arc<EthereumL1>,
        mev_boost: MevBoost,
        l2_slot_duration_sec: u64,
    ) -> Result<Self, Error> {
        let current_epoch = ethereum_l1.slot_clock.get_current_epoch()?;
        let operator = Operator::new(ethereum_l1.clone());
        Ok(Self {
            taiko,
            node_rx: Some(node_rx),
            node_to_p2p_tx,
            p2p_to_node_rx: Some(p2p_to_node_rx),
            gas_used: 0,
            ethereum_l1,
            _mev_boost: mev_boost,
            epoch: current_epoch,
            lookahead: vec![],
            l2_slot_duration_sec,
            preconfirmed_blocks: Arc::new(Mutex::new(HashMap::new())),
            operator,
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
        let taiko = self.taiko.clone();
        if let Some(node_rx) = self.node_rx.take() {
            let p2p_to_node_rx = self.p2p_to_node_rx.take().unwrap();
            tokio::spawn(async move {
                Self::handle_incoming_messages(
                    node_rx,
                    p2p_to_node_rx,
                    preconfirmed_blocks,
                    ethereum_l1,
                    taiko,
                )
                .await;
            });
        } else {
            tracing::error!("node_rx has already been moved");
        }
    }

    async fn handle_incoming_messages(
        mut node_rx: Receiver<BlockProposed>,
        mut p2p_to_node_rx: Receiver<Vec<u8>>,
        preconfirmed_blocks: Arc<Mutex<HashMap<u64, PreconfirmationProof>>>,
        ethereum_l1: Arc<EthereumL1>,
        taiko: Arc<Taiko>,
    ) {
        loop {
            tokio::select! {
                Some(block_proposed) = node_rx.recv() => {
                    tracing::debug!("Node received block proposed event: {:?}", block_proposed);
                    if let Err(e) = Self::check_preconfirmed_blocks_correctness(&preconfirmed_blocks, taiko.chain_id, &block_proposed, ethereum_l1.clone()).await {
                        tracing::error!("Failed to check preconfirmed blocks correctness: {}", e);
                    }
                    if let Err(e) = Self::clean_old_blocks(&preconfirmed_blocks, block_proposed.block_id).await {
                        tracing::error!("Failed to clean old blocks: {}", e);
                    }
                },
                Some(p2p_message) = p2p_to_node_rx.recv() => {
                    let msg: PreconfirmationMessage = p2p_message.into();
                    tracing::debug!("Node received message from p2p: {:?}", msg);
                    Self::check_preconfirmation_message(msg, &preconfirmed_blocks, ethereum_l1.clone(), taiko.clone()).await;
                }
            }
        }
    }

    async fn check_preconfirmation_message(
        msg: PreconfirmationMessage,
        preconfirmed_blocks: &Arc<Mutex<HashMap<u64, PreconfirmationProof>>>,
        ethereum_l1: Arc<EthereumL1>,
        taiko: Arc<Taiko>,
    ) {
        tracing::debug!("Node received message from p2p: {:?}", msg);
        // TODO check valid preconfer
        // check hash
        match L2TxListsCommit::from_preconf(msg.block_height, msg.tx_list_bytes, taiko.chain_id)
            .hash()
        {
            Ok(hash) => {
                if hash == msg.proof.commit_hash {
                    // check signature
                    match ethereum_l1
                        .execution_layer
                        .recover_address_from_msg(&msg.proof.commit_hash, &msg.proof.signature)
                    {
                        Ok(_) => {
                            // Add to preconfirmation map
                            preconfirmed_blocks
                                .lock()
                                .await
                                .insert(msg.block_height, msg.proof);
                            // Advance head
                            if let Err(e) = taiko
                                .advance_head_to_new_l2_block(msg.tx_lists, msg.gas_used)
                                .await
                            {
                                tracing::error!(
                                    "Failed to advance head: {} for block_id: {}",
                                    e,
                                    msg.block_height
                                );
                            }
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to check signature: {} for block_id: {}",
                                e,
                                msg.block_height
                            );
                        }
                    }
                } else {
                    tracing::warn!(
                        "Preconfirmatoin hash is not correct for block_id: {}",
                        msg.block_height
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Failed to calculate hash: {}", e);
            }
        }
    }

    async fn check_preconfirmed_blocks_correctness(
        preconfirmed_blocks: &Arc<Mutex<HashMap<u64, PreconfirmationProof>>>,
        chain_id: u64,
        block_proposed: &BlockProposed,
        ethereum_l1: Arc<EthereumL1>,
    ) -> Result<(), Error> {
        let preconfirmed_blocks = preconfirmed_blocks.lock().await;
        if let Some(block) = preconfirmed_blocks.get(&block_proposed.block_id) {
            //Signature is already verified on precof insertion
            if block.commit_hash != block_proposed.tx_list_hash {
                info!(
                    "Block tx_list_hash is not correct for block_id: {}. Calling proof of incorrect preconfirmation.",
                    block_proposed.block_id
                );
                ethereum_l1
                    .execution_layer
                    .prove_incorrect_preconfirmation(
                        block_proposed.block_id,
                        chain_id,
                        block.commit_hash,
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
        let current_epoch_timestamp = self
            .ethereum_l1
            .slot_clock
            .get_epoch_begin_timestamp(current_epoch)?;
        if current_epoch != self.epoch {
            tracing::debug!(
                "Current epoch changed from {} to {}",
                self.epoch,
                current_epoch
            );
            self.epoch = current_epoch;

            self.operator = Operator::new(self.ethereum_l1.clone());
            self.operator
                .update_preconfer_lookahead_for_epoch(current_epoch_timestamp, &self.lookahead)
                .await?;

            self.lookahead = self
                .ethereum_l1
                .consensus_layer
                .get_lookahead(self.epoch + 1)
                .await?;
        }

        let current_slot = self.ethereum_l1.slot_clock.get_current_slot()?;

        match self.operator.get_status(current_slot)? {
            OperatorStatus::PreconferAndProposer => {
                // TODO: replace with mev-boost forced inclusion list
                let (lookahead_pointer, lookahead_params) =
                    self.get_lookahead_params(current_epoch_timestamp).await?;
                self.preconfirm_block(lookahead_pointer, lookahead_params)
                    .await?;
            }
            OperatorStatus::Preconfer => {
                let (lookahead_pointer, lookahead_params) =
                    self.get_lookahead_params(current_epoch_timestamp).await?;
                self.preconfirm_block(lookahead_pointer, lookahead_params)
                    .await?;
            }
            OperatorStatus::None => {
                tracing::debug!("Not my slot to preconfirm: {}", current_slot);
            }
        }

        Ok(())
    }

    async fn get_lookahead_params(
        &mut self,
        current_epoch_timestamp: u64,
    ) -> Result<(u64, Vec<PreconfTaskManager::LookaheadSetParam>), Error> {
        if self
            .operator
            .should_post_lookahead(current_epoch_timestamp)
            .await?
        {
            let lookahead_params = self
                .ethereum_l1
                .execution_layer
                .get_lookahead_params_for_epoch_using_beacon_lookahead(
                    self.ethereum_l1
                        .slot_clock
                        .get_epoch_begin_timestamp(self.epoch + 1)?,
                    &self.lookahead,
                )
                .await?;

            let lookahead = self.ethereum_l1.execution_layer.get_lookahead().await?;
            let current_timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs();
            let lookahead_pointer = lookahead
                .iter()
                .position(|entry| {
                    entry.preconfer == self.ethereum_l1.execution_layer.get_preconfer_address()
                        && current_timestamp > entry.prevTimestamp
                        && current_timestamp <= entry.timestamp
                })
                .ok_or(anyhow::anyhow!(
                    "get_lookahead_params: Preconfer not found in lookahead"
                ))?;

            return Ok((lookahead_pointer as u64, lookahead_params));
        }

        Ok((0, vec![]))
    }

    async fn preconfirm_block(
        &mut self,
        lookahead_pointer: u64,
        lookahead_params: Vec<PreconfTaskManager::LookaheadSetParam>,
    ) -> Result<(), Error> {
        tracing::debug!(
            "Preconfirming for the slot: {:?}",
            self.ethereum_l1.slot_clock.get_current_slot()?
        );

        let pending_tx_lists = self.taiko.get_pending_l2_tx_lists().await?;
        if pending_tx_lists.tx_list_bytes.is_empty() {
            return Ok(());
        }

        let new_block_height = pending_tx_lists.parent_block_id + 1;
        let (commit_hash, signature) =
            self.generate_commit_hash_and_signature(&pending_tx_lists, new_block_height)?;

        let proof = PreconfirmationProof {
            commit_hash,
            signature,
        };
        let preconf_message = PreconfirmationMessage {
            block_height: new_block_height,
            tx_lists: pending_tx_lists.tx_lists.clone(),
            tx_list_bytes: pending_tx_lists.tx_list_bytes[0].clone(), //TODO: handle rest tx lists
            gas_used: self.gas_used,
            proof: proof.clone(),
        };
        self.send_preconfirmations_to_the_avs_p2p(preconf_message.clone())
            .await?;
        self.taiko
            .advance_head_to_new_l2_block(pending_tx_lists.tx_lists, self.gas_used)
            .await?;
        self.ethereum_l1
            .execution_layer
            .propose_new_block(
                pending_tx_lists.tx_list_bytes[0].clone(), //TODO: handle rest tx lists
                pending_tx_lists.parent_meta_hash,
                lookahead_pointer,
                lookahead_params,
            )
            .await?;

        self.preconfirmed_blocks
            .lock()
            .await
            .insert(new_block_height, proof);

        Ok(())
    }

    // TODO: use web3signer to sign the message
    fn generate_commit_hash_and_signature(
        &self,
        reply: &RPCReplyL2TxLists,
        block_height: u64,
    ) -> Result<([u8; 32], [u8; 65]), Error> {
        let commit = L2TxListsCommit::new(reply, block_height, self.taiko.chain_id);
        let hash = commit.hash()?;
        let signature = self
            .ethereum_l1
            .execution_layer
            .sign_message_with_private_ecdsa_key(&hash[..])?;
        Ok((hash, signature))
    }

    async fn clean_old_blocks(
        preconfirmed_blocks: &Arc<Mutex<HashMap<u64, PreconfirmationProof>>>,
        current_block_height: u64,
    ) -> Result<(), Error> {
        let oldest_block_to_keep = current_block_height - OLDEST_BLOCK_DISTANCE;
        let mut preconfirmed_blocks = preconfirmed_blocks.lock().await;
        preconfirmed_blocks.retain(|block_height, _| block_height >= &oldest_block_to_keep);
        Ok(())
    }

    async fn send_preconfirmations_to_the_avs_p2p(
        &self,
        message: PreconfirmationMessage,
    ) -> Result<(), Error> {
        self.node_to_p2p_tx
            .send(message.into())
            .await
            .map_err(|e| any_err!("Failed to send message to node_to_p2p_tx: {}", e))
    }
}
