pub mod block_proposed_receiver;
mod commit;
mod l2_block_id;
mod operator;
mod preconfirmation_helper;

use crate::{
    ethereum_l1::{block_proposed::BlockProposedV2, EthereumL1},
    taiko::{l2_tx_lists::RPCReplyL2TxLists, Taiko},
    utils::types::*,
};
use anyhow::Error;
use commit::L2TxListsCommit;
use l2_block_id::L2BlockId;
use operator::{Operator, Status as OperatorStatus};
use preconfirmation_helper::PreconfirmationHelper;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::{mpsc::Receiver, Mutex};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

const OLDEST_BLOCK_DISTANCE: u64 = 256;

pub struct Node {
    taiko: Arc<Taiko>,
    node_block_proposed_rx: Option<Receiver<BlockProposedV2>>,
    ethereum_l1: Arc<EthereumL1>,
    epoch: Epoch,
    l2_slot_duration_sec: u64,
    is_preconfer_now: Arc<AtomicBool>,
    preconfirmation_txs: Arc<Mutex<HashMap<u64, Vec<u8>>>>, // block_id -> tx
    operator: Operator,
    preconfirmation_helper: PreconfirmationHelper,
    l2_block_id: Arc<L2BlockId>,
}

impl Node {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        node_rx: Receiver<BlockProposedV2>,
        taiko: Arc<Taiko>,
        ethereum_l1: Arc<EthereumL1>,
        l2_slot_duration_sec: u64,
    ) -> Result<Self, Error> {
        let init_epoch = 0;
        let operator = Operator::new(ethereum_l1.clone())?;
        Ok(Self {
            taiko,
            node_block_proposed_rx: Some(node_rx),
            ethereum_l1,
            epoch: init_epoch,
            l2_slot_duration_sec,
            is_preconfer_now: Arc::new(AtomicBool::new(false)),
            preconfirmation_txs: Arc::new(Mutex::new(HashMap::new())),
            operator,
            preconfirmation_helper: PreconfirmationHelper::new(),
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
        let ethereum_l1 = self.ethereum_l1.clone();
        let taiko = self.taiko.clone();
        let is_preconfer_now = self.is_preconfer_now.clone();
        let preconfirmation_txs = self.preconfirmation_txs.clone();
        let l2_block_id = self.l2_block_id.clone();
        if let Some(node_rx) = self.node_block_proposed_rx.take() {
            tokio::spawn(async move {
                Self::handle_incoming_messages(
                    node_rx,
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
                    } else {
                        debug!("Node is Preconfer and received block proposed event: {:?}", block_proposed.block_id());
                        preconfirmation_txs.lock().await.remove(&block_proposed.block_id());
                    }
                },
            }
        }
    }

    async fn advance_l2_head(ethereum_l1: Arc<EthereumL1>, taiko: Arc<Taiko>) {
        // TODO replace that call with taiko call
    }

    async fn preconfirmation_loop(&mut self) {
        debug!("Main perconfirmation loop started");
        // Synchronize with L1 Slot Start Time
        let duration_to_next_slot = self.ethereum_l1.slot_clock.duration_to_next_slot().unwrap();
        sleep(duration_to_next_slot).await;

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
        let current_slot = self.ethereum_l1.slot_clock.get_current_slot()?;

        match self.operator.get_status().await? {
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
            }
            _ => unreachable!(),
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

        let pending_tx_lists = self.taiko.get_pending_l2_tx_lists().await?;
        if pending_tx_lists.tx_list_bytes.is_empty() {
            return Ok(());
        }

        debug!(
            "Pending {} transactions to preconfirm",
            pending_tx_lists.tx_list_bytes.len()
        );
        let pending_tx_lists_bytes = pending_tx_lists.tx_list_bytes[0].clone(); // TODO: handle multiple tx lists

        let new_block_height = self.l2_block_id.next(pending_tx_lists.parent_block_id);
        debug!("Preconfirming block with the height: {}", new_block_height);

        let (commit_hash, signature) =
            self.generate_commit_hash_and_signature(&pending_tx_lists, new_block_height)?;

        //self.send_preconfirmations_to_the_avs_p2p(preconf_message.clone());
        self.taiko
            .advance_head_to_new_l2_block(pending_tx_lists.tx_lists)
            .await?;

        // TODO get tx count
        // let tx_count = pending_tx_lists.count();
        let tx = self
            .ethereum_l1
            .execution_layer
            .propose_batch(
                self.preconfirmation_helper.get_next_nonce(),
                pending_tx_lists_bytes,
                1, //TODO replace with a correct tx count
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
