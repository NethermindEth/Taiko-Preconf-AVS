mod operator;
mod preconfirmation_helper;

use crate::{
    ethereum_l1::EthereumL1,
    taiko::{l2_tx_lists::PendingTxLists, Taiko},
};
use anyhow::Error;
use operator::{Operator, Status as OperatorStatus};
use preconfirmation_helper::PreconfirmationHelper;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

pub struct Node {
    taiko: Arc<Taiko>,
    ethereum_l1: Arc<EthereumL1>,
    preconf_heartbeat_ms: u64,
    operator: Operator,
    preconfirmation_helper: PreconfirmationHelper,
    pending_tx_lists_buffer: PendingTxLists,
    previous_status: OperatorStatus, // temporary to handle nonce issue
}

impl Node {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
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
            taiko,
            ethereum_l1,
            preconf_heartbeat_ms,
            operator,
            preconfirmation_helper: PreconfirmationHelper::new(),
            pending_tx_lists_buffer: PendingTxLists::new(),
            previous_status: OperatorStatus::None,
        })
    }

    /// Consumes the Node and starts two loops:
    /// one for handling incoming messages and one for the block preconfirmation
    pub async fn entrypoint(mut self) -> Result<(), Error> {
        info!("Starting node");
        self.handle_nonce_issue().await?;
        self.preconfirmation_loop().await;
        Ok(())
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
            OperatorStatus::PreconferAndL1Submitter => {
                self.preconfirm_and_submit_block().await?;
            }
            OperatorStatus::Preconfer => {
                self.preconfirm_block().await?;
            }
            OperatorStatus::PreconferHandoverBuffer(buffer_ms) => {
                tokio::time::sleep(Duration::from_millis(buffer_ms)).await;
                self.preconfirm_block().await?;
            }
            OperatorStatus::None => {
                info!(
                    "Not my slot to preconfirm, {}",
                    self.get_current_slots_info()?
                );
            }
            OperatorStatus::L1Submitter => {
                self.submit_left_txs().await?;
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

    async fn preconfirm_and_submit_block(&mut self) -> Result<(), Error> {
        info!(
            "Preconfirming and submitting for {}",
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

        if self.pending_tx_lists_buffer.is_empty() {
            self.pending_tx_lists_buffer = pending_tx_lists;
        } else {
            self.pending_tx_lists_buffer.extend(pending_tx_lists);
        }

        let next_nonce = self.preconfirmation_helper.get_next_nonce();
        self.ethereum_l1
            .execution_layer
            .send_batch_to_l1(
                std::mem::take(&mut self.pending_tx_lists_buffer),
                next_nonce,
            )
            .await?;

        Ok(())
    }

    async fn preconfirm_block(&mut self) -> Result<(), Error> {
        info!("Preconfirming for the {}", self.get_current_slots_info()?);

        let pending_tx_lists = self.taiko.get_pending_l2_tx_lists_from_taiko_geth().await?;
        if pending_tx_lists.is_empty() {
            debug!("No pending txs, skipping preconfirmation");
            return Ok(());
        }

        self.taiko
            .advance_head_to_new_l2_blocks(pending_tx_lists.clone())
            .await?;
        self.pending_tx_lists_buffer.extend(pending_tx_lists);

        Ok(())
    }

    async fn submit_left_txs(&mut self) -> Result<(), Error> {
        if self.pending_tx_lists_buffer.is_empty() {
            debug!("No pending txs, skipping submission");
            return Ok(());
        }

        info!("Submitting left {} txs", self.pending_tx_lists_buffer.len());

        self.ethereum_l1
            .execution_layer
            .send_batch_to_l1(
                std::mem::take(&mut self.pending_tx_lists_buffer),
                self.preconfirmation_helper.get_next_nonce(),
            )
            .await?;

        Ok(())
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
}
