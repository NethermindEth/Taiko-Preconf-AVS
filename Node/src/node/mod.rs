pub mod block_proposed_receiver;
mod commit;
mod l2_block_id;
pub mod lookahead_monitor;
pub mod lookahead_updated_receiver;
mod operator;
mod preconfirmation_helper;
mod preconfirmation_message;
mod preconfirmation_proof;

use crate::{
    bls::BLSService,
    ethereum_l1::{
        block_proposed::BlockProposedV2, execution_layer::PreconfTaskManager, EthereumL1,
    },
    mev_boost::MevBoost,
    taiko::{l2_tx_lists::RPCReplyL2TxLists, Taiko},
    utils::types::*,
};
use anyhow::Error;
use commit::L2TxListsCommit;
use l2_block_id::L2BlockId;
use operator::{Operator, Status as OperatorStatus};
use preconfirmation_helper::PreconfirmationHelper;
use preconfirmation_message::PreconfirmationMessage;
use preconfirmation_proof::PreconfirmationProof;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::{
    mpsc::{Receiver, Sender},
    Mutex,
};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

const OLDEST_BLOCK_DISTANCE: u64 = 256;

type PreconfirmedBlocks = Arc<Mutex<HashMap<u64, PreconfirmationMessage>>>;

pub struct Node {
    taiko: Arc<Taiko>,
    node_block_proposed_rx: Option<Receiver<BlockProposedV2>>,
    node_to_p2p_tx: Sender<Vec<u8>>,
    p2p_to_node_rx: Option<Receiver<Vec<u8>>>,
    ethereum_l1: Arc<EthereumL1>,
    mev_boost: MevBoost,
    epoch: Epoch,
    l2_slot_duration_sec: u64,
    preconfirmed_blocks: PreconfirmedBlocks,
    is_preconfer_now: Arc<AtomicBool>,
    preconfirmation_txs: Arc<Mutex<HashMap<u64, Vec<u8>>>>, // block_id -> tx
    operator: Operator,
    preconfirmation_helper: PreconfirmationHelper,
    bls_service: Arc<BLSService>,
    always_push_lookahead: bool,
    l2_block_id: Arc<L2BlockId>,
}

impl Node {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        node_rx: Receiver<BlockProposedV2>,
        node_to_p2p_tx: Sender<Vec<u8>>,
        p2p_to_node_rx: Receiver<Vec<u8>>,
        taiko: Arc<Taiko>,
        ethereum_l1: Arc<EthereumL1>,
        mev_boost: MevBoost,
        l2_slot_duration_sec: u64,
        bls_service: Arc<BLSService>,
        always_push_lookahead: bool,
    ) -> Result<Self, Error> {
        let init_epoch = 0;
        let operator = Operator::new(ethereum_l1.clone(), init_epoch)?;
        Ok(Self {
            taiko,
            node_block_proposed_rx: Some(node_rx),
            node_to_p2p_tx,
            p2p_to_node_rx: Some(p2p_to_node_rx),
            ethereum_l1,
            mev_boost,
            epoch: init_epoch,
            l2_slot_duration_sec,
            preconfirmed_blocks: Arc::new(Mutex::new(HashMap::new())),
            is_preconfer_now: Arc::new(AtomicBool::new(false)),
            preconfirmation_txs: Arc::new(Mutex::new(HashMap::new())),
            operator,
            preconfirmation_helper: PreconfirmationHelper::new(),
            bls_service,
            always_push_lookahead,
            l2_block_id: Arc::new(L2BlockId::new()),
        })
    }

    /// Consumes the Node and starts two loops:
    /// one for handling incoming messages and one for the block preconfirmation
    pub async fn entrypoint(mut self) -> Result<(), Error> {
        info!("Starting node");
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
        let l2_block_id = self.l2_block_id.clone();
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
                    l2_block_id,
                )
                .await;
            });
        } else {
            error!("Some of the node_rx, p2p_to_node_rx, or lookahead_updated_rx has already been moved");
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_incoming_messages(
        mut node_rx: Receiver<BlockProposedV2>,
        mut p2p_to_node_rx: Receiver<Vec<u8>>,
        preconfirmed_blocks: PreconfirmedBlocks,
        ethereum_l1: Arc<EthereumL1>,
        taiko: Arc<Taiko>,
        is_preconfer_now: Arc<AtomicBool>,
        preconfirmation_txs: Arc<Mutex<HashMap<u64, Vec<u8>>>>,
        l2_block_id: Arc<L2BlockId>,
    ) {
        loop {
            tokio::select! {
                Some(block_proposed) = node_rx.recv() => {
                    if !is_preconfer_now.load(Ordering::Acquire) {
                        debug!("Node received block proposed event: {:?}", block_proposed.block_id());
                        if let Err(e) = Self::check_preconfirmed_blocks_correctness(&preconfirmed_blocks, taiko.chain_id, &block_proposed, ethereum_l1.clone()).await {
                            error!("Failed to check preconfirmed blocks correctness: {}", e);
                        }
                        if let Err(e) = Self::clean_old_blocks(&preconfirmed_blocks, block_proposed.block_id()).await {
                            error!("Failed to clean old blocks: {}", e);
                        }
                    } else {
                        debug!("Node is Preconfer and received block proposed event: {:?}", block_proposed.block_id());
                        preconfirmation_txs.lock().await.remove(&block_proposed.block_id());
                    }
                },
                Some(p2p_message) = p2p_to_node_rx.recv() => {
                    if !is_preconfer_now.load(Ordering::Acquire) {
                        debug!("Received Message from p2p!");
                        let msg: PreconfirmationMessage = p2p_message.into();
                        l2_block_id.update(msg.block_height);
                        Self::advance_l2_head(msg, &preconfirmed_blocks, ethereum_l1.clone(), taiko.clone()).await;
                    } else {
                        debug!("Node is Preconfer and received message from p2p: {:?}", p2p_message);
                    }
                }
            }
        }
    }

    async fn is_valid_preconfer(
        ethereum_l1: Arc<EthereumL1>,
        preconfer: PreconferAddress,
    ) -> Result<(), Error> {
        // get current lookahead
        let epoch = ethereum_l1.slot_clock.get_current_epoch()?;

        let current_lookahead = ethereum_l1
            .execution_layer
            .get_lookahead_preconfer_addresses_for_epoch(epoch)
            .await?;

        // get slot number in epoch
        let slot_of_epoch = ethereum_l1.slot_clock.get_current_slot_of_epoch()?;
        debug!("slot_of_epoch: {}", slot_of_epoch);

        // get current preconfer
        if current_lookahead[slot_of_epoch as usize] == preconfer {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "is_valid_preconfer: P2P message Preconfer is not equal to current preconfer"
            ))
        }
    }

    async fn advance_l2_head(
        msg: PreconfirmationMessage,
        preconfirmed_blocks: &PreconfirmedBlocks,
        ethereum_l1: Arc<EthereumL1>,
        taiko: Arc<Taiko>,
    ) {
        // check hash
        let tx_list_commit =
            L2TxListsCommit::from_preconf(msg.block_height, msg.tx_list_hash, taiko.chain_id);
        debug!(
            "Match txListCommit, tx list hash: {}",
            hex::encode(msg.tx_list_hash)
        );
        match tx_list_commit.hash() {
            Ok(hash) => {
                if hash == msg.proof.commit_hash {
                    // check signature
                    match ethereum_l1
                        .execution_layer
                        .recover_address_from_msg(&msg.proof.commit_hash, &msg.proof.signature)
                    {
                        Ok(preconfer) => {
                            // check valid preconfer address
                            if let Err(e) =
                                Self::is_valid_preconfer(ethereum_l1.clone(), preconfer.into())
                                    .await
                            {
                                error!("Error: {} for block_id: {}", e, msg.block_height);
                                return;
                            }
                            // Add to preconfirmation map
                            debug!(
                                "Adding to preconfirmation map block_height: {}",
                                msg.block_height
                            );
                            preconfirmed_blocks
                                .lock()
                                .await
                                .insert(msg.block_height, msg.clone());
                            // Advance head
                            if let Err(e) = taiko.advance_head_to_new_l2_block(msg.tx_lists).await {
                                error!(
                                    "Failed to advance head: {} for block_id: {}",
                                    e, msg.block_height
                                );
                            }
                        }
                        Err(e) => {
                            error!(
                                "Failed to check signature: {} for block_id: {}",
                                e, msg.block_height
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
        preconfirmed_blocks: &PreconfirmedBlocks,
        chain_id: u64,
        block_proposed: &BlockProposedV2,
        ethereum_l1: Arc<EthereumL1>,
    ) -> Result<(), Error> {
        let preconfirmed_blocks = preconfirmed_blocks.lock().await;
        if let Some(preconf_block) = preconfirmed_blocks.get(&block_proposed.block_id()) {
            ethereum_l1
                .execution_layer
                .check_and_prove_incorrect_preconfirmation(
                    chain_id,
                    preconf_block.tx_list_hash,
                    preconf_block.proof.signature,
                    block_proposed,
                )
                .await?;
        } else {
            debug!(
                "No preconfirmed block with block_id: {}",
                block_proposed.block_id()
            );
        }
        Ok(())
    }

    async fn preconfirmation_loop(&mut self) {
        debug!("Main perconfirmation loop started");
        // Synchronize with L1 Slot Start Time
        let duration_to_next_slot = self.ethereum_l1.slot_clock.duration_to_next_slot().unwrap();
        sleep(duration_to_next_slot).await;

        // Setup protocol if needed
        if let Err(e) = self.operator.check_empty_lookahead().await {
            error!("Failed to initialize lookahead: {}", e);
        }

        // start preconfirmation loop
        let mut interval = tokio::time::interval(Duration::from_secs(self.l2_slot_duration_sec));
        loop {
            interval.tick().await;

            if let Err(err) = self.main_block_preconfirmation_step().await {
                error!("Failed to execute main block preconfirmation step: {}", err);
            }
        }
    }

    async fn main_block_preconfirmation_step(&mut self) -> Result<(), Error> {
        let current_epoch = self.ethereum_l1.slot_clock.get_current_epoch()?;
        if current_epoch != self.epoch {
            self.new_epoch_started(current_epoch).await?;
        }

        let current_slot = self.ethereum_l1.slot_clock.get_current_slot()?;

        match self.operator.get_status(current_slot).await? {
            OperatorStatus::PreconferAndProposer => {
                self.preconfirm_last_slot().await?;
            }
            OperatorStatus::Preconfer => {
                self.preconfirm_block(true).await?;
            }
            OperatorStatus::None => {
                info!(
                    "Not my slot to preconfirm. Epoch {}, slot: {} ({}), L2 slot: {}",
                    self.epoch,
                    current_slot,
                    self.ethereum_l1.slot_clock.slot_of_epoch(current_slot),
                    self.ethereum_l1
                        .slot_clock
                        .get_l2_slot_number_within_l1_slot()?
                );

                // Check if we need to push lookahead when we are not preconfer
                if self.always_push_lookahead {
                    self.operator.check_empty_lookahead().await?;
                }
            }
        }

        Ok(())
    }

    async fn new_epoch_started(&mut self, new_epoch: u64) -> Result<(), Error> {
        info!(
            "⏰ Current epoch changed from {} to {}",
            self.epoch, new_epoch
        );
        self.epoch = new_epoch;

        self.operator = Operator::new(self.ethereum_l1.clone(), new_epoch)?;
        // TODO it would be better to do it 1 epoch later
        self.operator.update_preconfer_lookahead_for_epoch().await?;
        // TODO
        #[cfg(debug_assertions)]
        self.operator
            .print_preconfer_slots(self.ethereum_l1.slot_clock.get_current_slot()?)
            .await;

        Ok(())
    }

    async fn get_lookahead_params(
        &mut self,
    ) -> Result<Option<Vec<PreconfTaskManager::LookaheadSetParam>>, Error> {
        if self.operator.should_post_lookahead_for_next_epoch().await? {
            debug!("Should post lookahead params, getting them");
            let cl_lookahead = self
                .ethereum_l1
                .consensus_layer
                .get_lookahead(self.epoch + 1)
                .await?;

            let lookahead_params = self
                .ethereum_l1
                .execution_layer
                .get_lookahead_params_for_epoch_using_cl_lookahead(self.epoch + 1, &cl_lookahead)
                .await?;

            debug!("Got Lookahead params: {}", lookahead_params.len());

            return Ok(Some(lookahead_params));
        }
        Ok(None)
    }

    async fn preconfirm_last_slot(&mut self) -> Result<(), Error> {
        debug!("Preconfirming last slot");
        self.preconfirm_block(false).await?;
        const FINAL_L2_SLOT_PERCONFIRMATION: u64 = 3;
        if self
            .ethereum_l1
            .slot_clock
            .get_l2_slot_number_within_l1_slot()?
            == FINAL_L2_SLOT_PERCONFIRMATION
        {
            debug!("Last(4th) perconfirmation in the last L1 slot for the preconfer");
            // Last(4th) perconfirmation when we are proposer and preconfer
            self.is_preconfer_now.store(false, Ordering::Release);

            let mut preconfirmation_txs = self.preconfirmation_txs.lock().await;
            if !preconfirmation_txs.is_empty() {
                debug!("Call MEV Boost for {} txs", preconfirmation_txs.len());
                // Build constraints
                let constraints: Vec<Vec<u8>> = preconfirmation_txs
                    .iter()
                    .map(|(_, value)| value.clone())
                    .collect();
                // Get slot_id
                let slot_id = self.ethereum_l1.slot_clock.get_current_slot()?;

                self.mev_boost
                    .force_inclusion(constraints, slot_id, self.bls_service.clone())
                    .await?;

                preconfirmation_txs.clear();
            }
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
        let current_slot = self.ethereum_l1.slot_clock.get_current_slot()?;
        info!(
            "Preconfirming for the epoch: {}, slot: {} ({}), L2 slot: {}",
            self.epoch,
            current_slot,
            self.ethereum_l1.slot_clock.slot_of_epoch(current_slot),
            self.ethereum_l1
                .slot_clock
                .get_l2_slot_number_within_l1_slot()?
        );

        if !self.is_preconfer_now.load(Ordering::Acquire) {
            self.is_preconfer_now.store(true, Ordering::Release);
            self.start_propose().await?;
        }

        let lookahead_params = self.get_lookahead_params().await?;
        let pending_tx_lists = self.taiko.get_pending_l2_tx_lists().await?;
        let pending_tx_lists_bytes = if pending_tx_lists.tx_list_bytes.is_empty() {
            if let Some(lookahead_params) = lookahead_params {
                if self.ethereum_l1.slot_clock.get_current_slot_of_epoch()? % 4 == 1 {
                    debug!("No pending transactions to preconfirm, force pushing lookahead");
                    if let Err(err) = self
                        .ethereum_l1
                        .execution_layer
                        .force_push_lookahead(lookahead_params)
                        .await
                    {
                        if err.to_string().contains("AlreadyKnown") {
                            debug!("Force push lookahead already known");
                        } else {
                            error!("Failed to force push lookahead: {}", err);
                        }
                    } else {
                        self.preconfirmation_helper.increment_nonce();
                    }
                }
            }
            // No transactions skip preconfirmation step
            return Ok(());
        } else {
            debug!(
                "Pending {} transactions to preconfirm",
                pending_tx_lists.tx_list_bytes.len()
            );
            pending_tx_lists.tx_list_bytes[0].clone() // TODO: handle multiple tx lists
        };

        let new_block_height = self.l2_block_id.next(pending_tx_lists.parent_block_id);
        debug!("Preconfirming block with the height: {}", new_block_height);

        let (commit_hash, signature) =
            self.generate_commit_hash_and_signature(&pending_tx_lists, new_block_height)?;

        let proof = PreconfirmationProof {
            commit_hash,
            signature,
        };
        let preconf_message = PreconfirmationMessage::new(
            new_block_height,
            pending_tx_lists.tx_lists.clone(),
            &pending_tx_lists_bytes,
            proof.clone(),
        );
        self.send_preconfirmations_to_the_avs_p2p(preconf_message.clone());
        self.taiko
            .advance_head_to_new_l2_block(pending_tx_lists.tx_lists)
            .await?;

        let lookahead_pointer = self.operator.get_lookahead_pointer(current_slot)?;
        let tx = self
            .ethereum_l1
            .execution_layer
            .propose_new_block(
                self.preconfirmation_helper.get_next_nonce(),
                pending_tx_lists_bytes,
                lookahead_pointer,
                lookahead_params.unwrap_or(vec![]),
                send_to_contract,
            )
            .await?;

        debug!(
            "Proposed new block, with hash {}",
            alloy::primitives::keccak256(&tx)
        );
        // insert transaction
        self.preconfirmation_txs
            .lock()
            .await
            .insert(new_block_height, tx);

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

    async fn clean_old_blocks<PreconfMessageType>(
        preconfirmed_blocks: &Arc<Mutex<HashMap<u64, PreconfMessageType>>>,
        current_block_height: u64,
    ) -> Result<(), Error> {
        let oldest_block_to_keep = current_block_height.saturating_sub(OLDEST_BLOCK_DISTANCE);
        let mut preconfirmed_blocks = preconfirmed_blocks.lock().await;
        preconfirmed_blocks.retain(|block_height, _| block_height >= &oldest_block_to_keep);
        Ok(())
    }

    fn send_preconfirmations_to_the_avs_p2p(&self, message: PreconfirmationMessage) {
        debug!(
            "Send message to p2p, tx list hash: {}",
            hex::encode(message.tx_list_hash)
        );

        if let Err(err) = self.node_to_p2p_tx.try_send(message.into()) {
            error!("Failed to send message to node_to_p2p_tx: {}", err);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tokio::sync::Mutex;

    #[tokio::test]
    async fn test_clean_old_blocks() {
        let preconfirmed_blocks = Arc::new(Mutex::new(HashMap::new()));
        {
            let mut blocks = preconfirmed_blocks.lock().await;
            blocks.insert(1u64, "abc".to_string());
            blocks.insert(2u64, "def".to_string());
            blocks.insert(300u64, "ghi".to_string());
            blocks.insert(301u64, "jkl".to_string());
        }

        {
            Node::clean_old_blocks(&preconfirmed_blocks, 10)
                .await
                .unwrap();
            let blocks = preconfirmed_blocks.lock().await;
            assert_eq!(blocks.len(), 4);
            assert!(blocks.contains_key(&1));
            assert!(blocks.contains_key(&2));
        }

        {
            Node::clean_old_blocks(&preconfirmed_blocks, 300)
                .await
                .unwrap();
            let blocks = preconfirmed_blocks.lock().await;
            assert_eq!(blocks.len(), 2);
            assert!(blocks.contains_key(&300));
            assert!(blocks.contains_key(&301));
        }
    }
}
