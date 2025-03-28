pub(crate) mod batch_manager;
mod operator;

use crate::{ethereum_l1::EthereumL1, taiko::Taiko};
use anyhow::Error;
use batch_manager::{BatchBuilderConfig, BatchManager};
use operator::{Operator, Status as OperatorStatus};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

pub struct Node {
    cancel_token: CancellationToken,
    ethereum_l1: Arc<EthereumL1>,
    preconf_heartbeat_ms: u64,
    operator: Operator,
    batch_manager: BatchManager,
}

impl Node {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        cancel_token: CancellationToken,
        taiko: Arc<Taiko>,
        ethereum_l1: Arc<EthereumL1>,
        preconf_heartbeat_ms: u64,
        handover_window_slots: u64,
        handover_start_buffer_ms: u64,
        l1_height_lag: u64,
        batch_builder_config: BatchBuilderConfig,
    ) -> Result<Self, Error> {
        info!(
            "Batch builder config:\n\
             max_bytes_size_of_batch: {}\n\
             max_blocks_per_batch: {}\n\
             l1_slot_duration_sec: {}\n\
             max_time_shift_between_blocks_sec: {}\n\
             max_anchor_height_offset: {}",
            batch_builder_config.max_bytes_size_of_batch,
            batch_builder_config.max_blocks_per_batch,
            batch_builder_config.l1_slot_duration_sec,
            batch_builder_config.max_time_shift_between_blocks_sec,
            batch_builder_config.max_anchor_height_offset,
        );
        let operator = Operator::new(
            ethereum_l1.clone(),
            handover_window_slots,
            handover_start_buffer_ms,
        )?;
        Ok(Self {
            cancel_token,
            batch_manager: BatchManager::new(
                l1_height_lag,
                batch_builder_config,
                ethereum_l1.clone(),
                taiko,
            ),
            ethereum_l1,
            preconf_heartbeat_ms,
            operator,
        })
    }

    /// Consumes the Node and starts two loops:
    /// one for handling incoming messages and one for the block preconfirmation
    pub fn entrypoint(mut self) {
        info!("Starting node");
        tokio::spawn(async move {
            match self.warmup().await {
                Ok(()) => {
                    info!("Node warmup successful");
                }
                Err(err) => {
                    error!("Failed to startup node: {}", err);
                }
            }
            self.preconfirmation_loop().await;
        });
    }

    async fn warmup(&mut self) -> Result<(), Error> {
        info!("Warmup node");
        let current_status = self.operator.get_status().await?;

        // TODO check that when we are Preconfer or PreconferHandoverBuffer we will sync our l2 state on epoch boundry
        if matches!(
            current_status,
            OperatorStatus::None
                | OperatorStatus::Preconfer
                | OperatorStatus::PreconferHandoverBuffer(_)
        ) {
            info!("Status: {:?}, no need for warmup", current_status);
            return Ok(());
        }

        let height_taiko_inbox = self
            .ethereum_l1
            .execution_layer
            .get_l2_height_from_taiko_inbox()
            .await?;
        let height_taiko_geth = self.batch_manager.taiko.get_latest_l2_block_id().await?;
        info!("Height Taiko Inbox: {height_taiko_inbox}, Height Taiko Geth: {height_taiko_geth}");

        if height_taiko_inbox == height_taiko_geth {
            return Ok(());
        } else if height_taiko_inbox > height_taiko_geth {
            panic!("Taiko Geth is not synchronized with L1");
        } else {
            // height_taiko_inbox < height_taiko_geth
            // we have unprocessed L2 blocks
            // check if there is a pending tx on the mempool from your address
            let latest_nonce: u64 = self
                .ethereum_l1
                .execution_layer
                .get_preconfer_latest_nonce()
                .await?;
            let pending_nonce: u64 = self
                .ethereum_l1
                .execution_layer
                .get_preconfer_pending_nonce()
                .await?;
            info!("Latest nonce: {latest_nonce}, Pending nonce: {pending_nonce}");
            //if not, then read blocks from L2 execution to form your buffer (L2 batch) and continue operations normally
            // we didn't propose blocks to mempool
            if latest_nonce == pending_nonce {
                // The first block anchor id is valid, so we can continue.
                if self
                    .batch_manager
                    .is_block_valid(height_taiko_inbox + 1)
                    .await?
                {
                    // recover all missed l2 blocks
                    for current_height in height_taiko_inbox + 1..=height_taiko_geth {
                        self.batch_manager
                            .recover_from_l2_block(current_height)
                            .await?;
                    }
                    // TODO calculate batch params and decide is it possible to continue with it, be careful with timeShift
                    // Sould be fixed with https://github.com/NethermindEth/Taiko-Preconf-AVS/issues/303
                    // Now just submit all the batches
                    self.batch_manager.submit_batches(false).await?;
                } else {
                    // The first block anchor id is not valid
                    // TODO reorg + reanchor + preconfirm again
                    // Just do force reorg
                    self.batch_manager
                        .taiko
                        .trigger_l2_reorg(height_taiko_inbox)
                        .await?;
                }
            }
            //if yes, then continue operations normally without rebuilding the buffer
            // TODO handle gracefully
        }

        Ok(())
    }

    async fn preconfirmation_loop(&mut self) {
        debug!("Main perconfirmation loop started");
        // Synchronize with L1 Slot Start Time
        match self.ethereum_l1.slot_clock.duration_to_next_slot() {
            Ok(duration) => {
                sleep(duration).await;
            }
            Err(err) => {
                error!("Failed to get duration to next slot: {}", err);
            }
        }

        // start preconfirmation loop
        let mut interval = tokio::time::interval(Duration::from_millis(self.preconf_heartbeat_ms));
        // fix for handover buffer longer than l2 heart beat, keeps the loop synced
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            if self.cancel_token.is_cancelled() {
                info!("Shutdown signal received, exiting main loop...");
                if let Err(err) = self.batch_manager.submit_batches(false).await {
                    error!("Failed to submit batches at the application shut down: {err}");
                }
                return;
            }

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
                info!("Submitting left batches {}", self.get_current_slots_info()?);
                self.batch_manager.submit_batches(false).await?;
            }
            OperatorStatus::None => {
                info!(
                    "Not my slot to preconfirm {}",
                    self.get_current_slots_info()?
                );
                if self.batch_manager.has_batches() {
                    // TODO: Handle this situation gracefully
                    self.batch_manager.reset_builder();
                    warn!("Some batches were not successfully sent in the submitter window. Resetting batch builder.");
                }
            }
        }

        Ok(())
    }

    async fn preconfirm_block(&mut self, submit: bool) -> Result<(), Error> {
        info!(
            "Preconfirming {}{}",
            if submit { "and submitting " } else { "" },
            self.get_current_slots_info()?
        );

        self.batch_manager.preconfirm_block(submit).await
    }

    fn get_current_slots_info(&self) -> Result<String, Error> {
        let current_slot = self.ethereum_l1.slot_clock.get_current_slot()?;
        Ok(format!(
            "\t epoch: {}\t| slot: {} ({})\t| L2 slot: {}",
            self.ethereum_l1.slot_clock.get_current_epoch()?,
            current_slot,
            self.ethereum_l1.slot_clock.slot_of_epoch(current_slot),
            self.ethereum_l1
                .slot_clock
                .get_l2_slot_number_within_l1_slot()?
        ))
    }
}
