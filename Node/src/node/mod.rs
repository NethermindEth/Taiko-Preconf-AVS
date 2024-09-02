use crate::{
    bls::BLSService,
    ethereum_l1::{execution_layer::PreconfTaskManager, EthereumL1},
    mev_boost::MevBoost,
    taiko::{l2_tx_lists::RPCReplyL2TxLists, Taiko},
    utils::{
        block_proposed::BlockProposed, commit::L2TxListsCommit,
        preconfirmation_message::PreconfirmationMessage,
        preconfirmation_proof::PreconfirmationProof, types::*,
    },
};
use anyhow::{anyhow as any_err, Error};
use beacon_api_client::ProposerDuty;
use operator::{Operator, Status as OperatorStatus};
use std::sync::atomic::Ordering;
use std::{
    collections::HashMap,
    sync::{atomic::AtomicBool, Arc},
};
use tokio::sync::{
    mpsc::{Receiver, Sender},
    Mutex,
};
use tokio::time::{sleep, Duration};
use tracing::info;

pub mod block_proposed_receiver;
pub mod lookahead_updated_receiver;
mod operator;
mod preconfirmation_helper;
use preconfirmation_helper::PreconfirmationHelper;

const OLDEST_BLOCK_DISTANCE: u64 = 256;

pub struct Node {
    taiko: Arc<Taiko>,
    node_block_proposed_rx: Option<Receiver<BlockProposed>>,
    node_to_p2p_tx: Sender<Vec<u8>>,
    p2p_to_node_rx: Option<Receiver<Vec<u8>>>,
    gas_used: u64,
    ethereum_l1: Arc<EthereumL1>,
    mev_boost: MevBoost,
    epoch: Epoch,
    cl_lookahead: Vec<ProposerDuty>,
    lookahead_preconfer_buffer: Option<[PreconfTaskManager::LookaheadEntry; 64]>,
    l2_slot_duration_sec: u64,
    preconfirmed_blocks: Arc<Mutex<HashMap<u64, PreconfirmationProof>>>,
    is_preconfer_now: Arc<AtomicBool>,
    preconfirmation_txs: Arc<Mutex<HashMap<u64, Vec<u8>>>>, // block_id -> tx
    operator: Operator,
    preconfirmation_helper: PreconfirmationHelper,
    bls_service: Arc<BLSService>,
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
        bls_service: Arc<BLSService>,
    ) -> Result<Self, Error> {
        let current_epoch = ethereum_l1.slot_clock.get_current_epoch()?;
        let operator = Operator::new(ethereum_l1.clone());
        Ok(Self {
            taiko,
            node_block_proposed_rx: Some(node_rx),
            node_to_p2p_tx,
            p2p_to_node_rx: Some(p2p_to_node_rx),
            gas_used: 0,
            ethereum_l1,
            mev_boost,
            epoch: current_epoch,
            cl_lookahead: vec![],
            lookahead_preconfer_buffer: None,
            l2_slot_duration_sec,
            preconfirmed_blocks: Arc::new(Mutex::new(HashMap::new())),
            is_preconfer_now: Arc::new(AtomicBool::new(false)),
            preconfirmation_txs: Arc::new(Mutex::new(HashMap::new())),
            operator,
            preconfirmation_helper: PreconfirmationHelper::new(),
            bls_service,
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
        let is_preconfer_now = self.is_preconfer_now.clone();
        let preconfirmation_txs = self.preconfirmation_txs.clone();
        if let (Some(node_rx), Some(p2p_to_node_rx)) = (
            self.node_block_proposed_rx.take(),
            self.p2p_to_node_rx.take(),
        ) {
            tokio::spawn(async move {
                Self::handle_incoming_messages(
                    node_rx,
                    p2p_to_node_rx,
                    preconfirmed_blocks,
                    ethereum_l1,
                    taiko,
                    is_preconfer_now,
                    preconfirmation_txs,
                )
                .await;
            });
        } else {
            tracing::error!("Some of the node_rx, p2p_to_node_rx, or lookahead_updated_rx has already been moved");
        }
    }

    async fn handle_incoming_messages(
        mut node_rx: Receiver<BlockProposed>,
        mut p2p_to_node_rx: Receiver<Vec<u8>>,
        preconfirmed_blocks: Arc<Mutex<HashMap<u64, PreconfirmationProof>>>,
        ethereum_l1: Arc<EthereumL1>,
        taiko: Arc<Taiko>,
        is_preconfer_now: Arc<AtomicBool>,
        preconfirmation_txs: Arc<Mutex<HashMap<u64, Vec<u8>>>>,
    ) {
        loop {
            tokio::select! {
                Some(block_proposed) = node_rx.recv() => {
                    if !is_preconfer_now.load(Ordering::Acquire) {
                        tracing::debug!("Node received block proposed event: {:?}", block_proposed);
                        if let Err(e) = Self::check_preconfirmed_blocks_correctness(&preconfirmed_blocks, taiko.chain_id, &block_proposed, ethereum_l1.clone()).await {
                            tracing::error!("Failed to check preconfirmed blocks correctness: {}", e);
                        }
                        if let Err(e) = Self::clean_old_blocks(&preconfirmed_blocks, block_proposed.block_id).await {
                            tracing::error!("Failed to clean old blocks: {}", e);
                        }
                    } else {
                        tracing::debug!("Node is Preconfer and received block proposed event: {:?}", block_proposed);
                        preconfirmation_txs.lock().await.remove(&block_proposed.block_id);
                    }
                },
                Some(p2p_message) = p2p_to_node_rx.recv() => {
                    if !is_preconfer_now.load(Ordering::Acquire) {
                        let msg: PreconfirmationMessage = p2p_message.into();
                        tracing::debug!("Node received message from p2p: {:?}", msg);
                        Self::check_preconfirmation_message(msg, &preconfirmed_blocks, ethereum_l1.clone(), taiko.clone()).await;
                    } else {
                        tracing::debug!("Node is Preconfer and received message from p2p: {:?}", p2p_message);
                    }
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
        // Synchronize with L1 Slot Start Time
        let duration_to_next_slot = self.ethereum_l1.slot_clock.duration_to_next_slot().unwrap();
        sleep(duration_to_next_slot).await;
        // start preconfirmation loop
        let mut interval = tokio::time::interval(Duration::from_secs(self.l2_slot_duration_sec));
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
            self.new_epoch_started(current_epoch).await?;
        }

        let current_slot = self.ethereum_l1.slot_clock.get_current_slot()?;

        match self.operator.get_status(current_slot)? {
            OperatorStatus::PreconferAndProposer => {
                self.preconfirm_last_slot().await?;
            }
            OperatorStatus::Preconfer => {
                if !self.is_preconfer_now.load(Ordering::Acquire) {
                    self.is_preconfer_now.store(true, Ordering::Release);
                    self.start_propose().await?;
                }
                self.preconfirm_block(true).await?;
            }
            OperatorStatus::None => {
                tracing::debug!("Not my slot to preconfirm: {}", current_slot);
            }
        }

        Ok(())
    }

    async fn new_epoch_started(&mut self, new_epoch: u64) -> Result<(), Error> {
        tracing::debug!("Current epoch changed from {} to {}", self.epoch, new_epoch);
        let new_epoch_timestamp = self
            .ethereum_l1
            .slot_clock
            .get_epoch_begin_timestamp(new_epoch)?;

        self.epoch = new_epoch;

        self.operator = Operator::new(self.ethereum_l1.clone());
        self.operator
            .update_preconfer_lookahead_for_epoch(new_epoch_timestamp, &self.cl_lookahead)
            .await?;

        self.cl_lookahead = self
            .ethereum_l1
            .consensus_layer
            .get_lookahead(self.epoch + 1)
            .await?;
        self.lookahead_preconfer_buffer = Some(
            self.ethereum_l1
                .execution_layer
                .get_lookahead_preconfer_buffer()
                .await?,
        );

        Ok(())
    }

    async fn get_lookahead_params(
        &mut self,
        current_epoch_timestamp: u64,
    ) -> Result<(u64, Vec<PreconfTaskManager::LookaheadSetParam>), Error> {
        let current_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs();

        let lookahead_pointer = self
            .lookahead_preconfer_buffer
            .as_ref()
            .ok_or(anyhow::anyhow!(
                "get_lookahead_params: lookahead_preconfer_buffer is None"
            ))?
            .iter()
            .position(|entry| {
                entry.preconfer == self.ethereum_l1.execution_layer.get_preconfer_address()
                    && current_timestamp > entry.prevTimestamp
                    && current_timestamp <= entry.timestamp
            })
            .ok_or(anyhow::anyhow!(
                "get_lookahead_params: Preconfer not found in lookahead"
            ))? as u64;

        if self
            .operator
            .should_post_lookahead(current_epoch_timestamp)
            .await?
        {
            let lookahead_params = self
                .ethereum_l1
                .execution_layer
                .get_lookahead_params_for_epoch_using_cl_lookahead(
                    self.ethereum_l1
                        .slot_clock
                        .get_epoch_begin_timestamp(self.epoch + 1)?,
                    &self.cl_lookahead,
                )
                .await?;

            return Ok((lookahead_pointer, lookahead_params));
        }

        Ok((lookahead_pointer, vec![]))
    }

    async fn preconfirm_last_slot(&mut self) -> Result<(), Error> {
        self.preconfirm_block(false).await?;
        if self
            .preconfirmation_helper
            .is_last_final_slot_perconfirmation()
        {
            // Last(4th) perconfirmation when we are proposer and preconfer
            self.is_preconfer_now.store(false, Ordering::Release);

            let mut preconfirmation_txs = self.preconfirmation_txs.lock().await;
            if !preconfirmation_txs.is_empty() {
                // Build constraints
                let constraints: Vec<Vec<u8>> = preconfirmation_txs
                    .iter()
                    .map(|(_, value)| value.clone())
                    .collect();

                self.mev_boost
                    .force_inclusion(
                        constraints,
                        self.ethereum_l1.clone(),
                        self.bls_service.clone(),
                    )
                    .await?;

                preconfirmation_txs.clear();
            }
        } else {
            // Increment perconfirmations count when we are proposer and preconfer
            self.preconfirmation_helper
                .increment_final_slot_perconfirmation();
        }

        Ok(())
    }

    async fn start_propose(&mut self) -> Result<(), Error> {
        // get L1 preconfer wallet nonce
        let nonce = self
            .ethereum_l1
            .execution_layer
            .get_preconfer_nonce()
            .await?;

        self.preconfirmation_helper.init(nonce);
        Ok(())
    }

    async fn preconfirm_block(&mut self, send_to_contract: bool) -> Result<(), Error> {
        tracing::debug!(
            "Preconfirming for the slot: {:?}",
            self.ethereum_l1.slot_clock.get_current_slot()?
        );

        let current_epoch_timestamp = self
            .ethereum_l1
            .slot_clock
            .get_epoch_begin_timestamp(self.epoch)?;
        let (lookahead_pointer, lookahead_params) =
            self.get_lookahead_params(current_epoch_timestamp).await?;

        let pending_tx_lists = self.taiko.get_pending_l2_tx_lists().await?;
        if pending_tx_lists.tx_list_bytes.is_empty() {
            return Ok(());
        }

        let new_block_height = pending_tx_lists.parent_block_id + 1;
        let nonce = self.preconfirmation_helper.get_next_nonce();

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
        let tx = self
            .ethereum_l1
            .execution_layer
            .propose_new_block(
                nonce,
                pending_tx_lists.tx_list_bytes[0].clone(), //TODO: handle rest tx lists
                pending_tx_lists.parent_meta_hash,
                lookahead_pointer,
                lookahead_params,
                send_to_contract,
            )
            .await?;

        // insert transaction
        self.preconfirmation_txs
            .lock()
            .await
            .insert(new_block_height, tx);

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
