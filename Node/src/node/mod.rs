mod batch_proposer;
pub mod block_proposed_receiver;
mod commit;
mod operator;
mod preconfirmation_helper;

use crate::{
    ethereum_l1::{block_proposed::BlockProposedV2, EthereumL1},
    taiko::{
        l2_tx_lists::{PendingTxLists, RPCReplyL2TxLists},
        Taiko,
    },
};
use anyhow::Error;
use commit::L2TxListsCommit;
use operator::{Operator, Status as OperatorStatus};
use preconfirmation_helper::PreconfirmationHelper;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{mpsc::Receiver, Mutex};
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

pub struct Node {
    taiko: Arc<Taiko>,
    node_block_proposed_rx: Option<Receiver<BlockProposedV2>>,
    ethereum_l1: Arc<EthereumL1>,
    preconf_heartbeat_ms: u64,
    preconfirmation_txs: Arc<Mutex<HashMap<u64, Vec<u8>>>>, // block_id -> tx
    operator: Operator,
    preconfirmation_helper: PreconfirmationHelper,
    previous_status: OperatorStatus, // temporary to handle nonce issue
    batch_proposer: batch_proposer::BatchProposer,
}

impl Node {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        node_rx: Receiver<BlockProposedV2>,
        taiko: Arc<Taiko>,
        ethereum_l1: Arc<EthereumL1>,
        preconf_heartbeat_ms: u64,
        handover_window_slots: u64,
        handover_start_buffer_ms: u64,
    ) -> Result<Self, Error> {
        let operator = Operator::new(
            ethereum_l1.clone(),
            handover_window_slots,
            handover_start_buffer_ms,
        )?;
        Ok(Self {
            batch_proposer: batch_proposer::BatchProposer::new(ethereum_l1.clone()),
            taiko,
            node_block_proposed_rx: Some(node_rx),
            ethereum_l1,
            preconf_heartbeat_ms,
            preconfirmation_txs: Arc::new(Mutex::new(HashMap::new())),
            operator,
            preconfirmation_helper: PreconfirmationHelper::new(),
            previous_status: OperatorStatus::None,
        })
    }

    /// Consumes the Node and starts two loops:
    /// one for handling incoming messages and one for the block preconfirmation
    pub async fn entrypoint(mut self) -> Result<(), Error> {
        info!("Starting node");
        self.handle_nonce_issue().await?;
        self.start_new_msg_receiver_thread();
        self.preconfirmation_loop().await;
        Ok(())
    }

    fn start_new_msg_receiver_thread(&mut self) {
        let preconfirmation_txs = self.preconfirmation_txs.clone();
        if let Some(node_rx) = self.node_block_proposed_rx.take() {
            tokio::spawn(async move {
                Self::handle_incoming_messages(node_rx, preconfirmation_txs).await;
            });
        } else {
            error!("Some of the node_rx, p2p_to_node_rx, or lookahead_updated_rx has already been moved");
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_incoming_messages(
        mut node_rx: Receiver<BlockProposedV2>,
        preconfirmation_txs: Arc<Mutex<HashMap<u64, Vec<u8>>>>,
    ) {
        loop {
            tokio::select! {
                Some(block_proposed) = node_rx.recv() => {
                        debug!("Received block proposed event: {:?}", block_proposed.block_id());
                        preconfirmation_txs.lock().await.remove(&block_proposed.block_id());
                },
            }
        }
    }

    async fn preconfirmation_loop(&mut self) {
        debug!("Main perconfirmation loop started");
        // Synchronize with L1 Slot Start Time
        let duration_to_next_slot = self.ethereum_l1.slot_clock.duration_to_next_slot().unwrap();
        sleep(duration_to_next_slot).await;

        // start preconfirmation loop
        let mut interval = tokio::time::interval(Duration::from_millis(self.preconf_heartbeat_ms));
        loop {
            interval.tick().await;

            if let Err(err) = self.main_block_preconfirmation_step().await {
                error!("Failed to execute main block preconfirmation step: {}", err);
            }
        }
    }

    async fn main_block_preconfirmation_step(&mut self) -> Result<(), Error> {
        let current_status = self.operator.get_status().await?;
        if current_status != self.previous_status {
            self.previous_status = current_status.clone();
            self.handle_nonce_issue().await?;
        }

        match current_status {
            OperatorStatus::PreconferHandoverBuffer(buffer_ms) => {
                tokio::time::sleep(Duration::from_millis(buffer_ms)).await;
                self.preconfirm_block(false).await?;
            }
            OperatorStatus::Preconfer => {
                self.preconfirm_block(false).await?;
            }
            OperatorStatus::PreconferAndL1Submitter => {
                self.preconfirm_block(true).await?;
            }
            OperatorStatus::L1Submitter => {
                self.batch_proposer.submit_all().await?;
            }
            OperatorStatus::None => {
                info!(
                    "Not my slot to preconfirm, {}",
                    self.get_current_slots_info()?
                );
            }
        }

        Ok(())
    }

    // temporary workaround to handle nonce issue
    async fn handle_nonce_issue(&mut self) -> Result<(), Error> {
        let nonce = self
            .ethereum_l1
            .execution_layer
            .get_preconfer_nonce()
            .await?;
        self.preconfirmation_helper.init(nonce);
        Ok(())
    }

    async fn preconfirm_block(&mut self, submit: bool) -> Result<(), Error> {
        info!(
            "Preconfirming (submit: {}) for the {}",
            submit,
            self.get_current_slots_info()?
        );

        let pending_tx_lists = self.taiko.get_pending_l2_tx_lists_from_taiko_geth().await?;
        if pending_tx_lists.is_empty() {
            debug!("No pending txs, skipping preconfirmation");
            return Ok(());
        }

        self.taiko
            .advance_head_to_new_l2_blocks(pending_tx_lists.clone())
            .await?;

        self.batch_proposer
            .handle_l2_blocks(pending_tx_lists, submit)
            .await
    }

    fn get_current_slots_info(&self) -> Result<String, Error> {
        let current_slot = self.ethereum_l1.slot_clock.get_current_slot()?;
        Ok(format!(
            "epoch: {}, slot: {} ({}), L2 slot: {}",
            self.ethereum_l1.slot_clock.get_current_epoch()?,
            current_slot,
            self.ethereum_l1.slot_clock.slot_of_epoch(current_slot),
            self.ethereum_l1
                .slot_clock
                .get_l2_slot_number_within_l1_slot()?
        ))
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
}
