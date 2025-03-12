mod batch_builder;
mod operator;

use crate::{ethereum_l1::EthereumL1, shared::l2_block::L2Block, taiko::Taiko};
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
    batch_builder: batch_builder::BatchBuilder,
    l1_height_lag: u64,
}

impl Node {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        taiko: Arc<Taiko>,
        ethereum_l1: Arc<EthereumL1>,
        preconf_heartbeat_ms: u64,
        handover_window_slots: u64,
        handover_start_buffer_ms: u64,
        l1_height_lag: u64,
    ) -> Result<Self, Error> {
        let operator = Operator::new(
            ethereum_l1.clone(),
            handover_window_slots,
            handover_start_buffer_ms,
        )?;
        Ok(Self {
            batch_builder: batch_builder::BatchBuilder::new(),
            taiko,
            ethereum_l1,
            preconf_heartbeat_ms,
            operator,
            l1_height_lag,
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
                if let Some(batch) = self.batch_builder.get_batch() {
                    self.ethereum_l1
                        .execution_layer
                        .send_batch_to_l1(
                            batch.l2_blocks,
                            batch.anchor_block_id,
                            batch.timestamp_sec,
                        )
                        .await?;
                }
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

        if let Some(pending_tx_list) = self.taiko.get_pending_l2_tx_list_from_taiko_geth().await? {
            debug!(
                "Received pending tx list length: {}, bytes length: {}",
                pending_tx_list.tx_list.len(),
                pending_tx_list.bytes_length
            );
            let preconfirmation_timestamp =
                self.ethereum_l1.slot_clock.get_l2_slot_begin_timestamp()?;
            let l2_block = L2Block::new_from(pending_tx_list, preconfirmation_timestamp);

            if self.batch_builder.can_consume_l2_block(&l2_block) {
                if self.batch_builder.is_new_batch() {
                    self.batch_builder.set_anchor_id_and_timestamp(
                        self.get_anchor_block_id().await?,
                        preconfirmation_timestamp,
                    );
                }
                self.taiko
                    .advance_head_to_new_l2_block(
                        l2_block.clone(),
                        self.batch_builder.get_anchor_block_id(),
                    )
                    .await?;
                self.batch_builder.add_l2_block(l2_block);
                if submit && self.batch_builder.is_batch_full() {
                    self.submit_batch().await?;
                }
            } else if submit {
                self.submit_batch().await?;
            }
        } else {
            debug!("No pending txs, skipping preconfirmation");
        }

        Ok(())
    }

    async fn submit_batch(&mut self) -> Result<(), Error> {
        debug!("Submitting batch");
        if let Some(batch) = self.batch_builder.get_batch() {
            let last_block_timestamp = batch.get_last_l2_block_timestamp();
            self.ethereum_l1
                .execution_layer
                .send_batch_to_l1(batch.l2_blocks, batch.anchor_block_id, last_block_timestamp)
                .await
        } else {
            Ok(())
        }
    }

    async fn get_anchor_block_id(&self) -> Result<u64, Error> {
        let height_from_last_batch = self
            .ethereum_l1
            .execution_layer
            .get_anchor_block_id()
            .await?;
        let l1_height = self.ethereum_l1.execution_layer.get_l1_height().await?;
        let l1_height_with_lag = l1_height - self.l1_height_lag;

        Ok(std::cmp::max(height_from_last_batch, l1_height_with_lag))
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
