mod batch_proposer;
mod operator;

use crate::{
    ethereum_l1::EthereumL1,
    taiko::Taiko,
};
use anyhow::Error;
use operator::{Operator, Status as OperatorStatus};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info};

pub struct Node {
    taiko: Arc<Taiko>,
    ethereum_l1: Arc<EthereumL1>,
    preconf_heartbeat_ms: u64,
    operator: Operator,
    batch_proposer: batch_proposer::BatchProposer,
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
            batch_proposer: batch_proposer::BatchProposer::new(ethereum_l1.clone()),
            taiko,
            ethereum_l1,
            preconf_heartbeat_ms,
            operator,
        })
    }

    /// Consumes the Node and starts two loops:
    /// one for handling incoming messages and one for the block preconfirmation
    pub async fn entrypoint(mut self) -> Result<(), Error> {
        info!("Starting node");
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
}
