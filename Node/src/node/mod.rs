pub(crate) mod batch_manager;
mod operator;

use crate::{
    ethereum_l1::EthereumL1,
    shared::{l2_slot_info::L2SlotInfo, l2_tx_lists::PreBuiltTxList},
    taiko::Taiko,
};
use alloy::primitives::U256;
use anyhow::Error;
use batch_manager::{BatchBuilderConfig, BatchManager};
use operator::{Operator, Status as OperatorStatus};
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, trace, warn};

pub struct Thresholds {
    pub eth: U256,
    pub taiko: U256,
}

pub struct Node {
    cancel_token: CancellationToken,
    ethereum_l1: Arc<EthereumL1>,
    preconf_heartbeat_ms: u64,
    operator: Operator,
    batch_manager: BatchManager,
    thresholds: Thresholds,
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
        thresholds: Thresholds,
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
            thresholds,
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
                    // TODO change to panic
                    error!("Failed to warmup node: {}", err);
                }
            }
            self.preconfirmation_loop().await;
        });
    }

    async fn get_current_protocol_height(&self) -> Result<(u64, u64), Error> {
        let taiko_inbox_height = self
            .ethereum_l1
            .execution_layer
            .get_l2_height_from_taiko_inbox()
            .await?;

        let taiko_geth_height = self.batch_manager.taiko.get_latest_l2_block_id().await?;

        Ok((taiko_inbox_height, taiko_geth_height))
    }

    async fn warmup(&mut self) -> Result<(), Error> {
        info!("Warmup node");

        // Check TAIKO TOKEN balance
        let bond_balance = self
            .ethereum_l1
            .execution_layer
            .get_preconfer_inbox_bonds()
            .await
            .map_err(|e| Error::msg(format!("Failed to fetch bond balance: {}", e)))?;

        let wallet_balance = self
            .ethereum_l1
            .execution_layer
            .get_preconfer_wallet_bonds()
            .await
            .map_err(|e| Error::msg(format!("Failed to fetch bond balance: {}", e)))?;

        let total_balance = bond_balance + wallet_balance;

        if total_balance < self.thresholds.taiko {
            anyhow::bail!(
                "Total balance ({}) is below the required threshold ({})",
                total_balance,
                self.thresholds.taiko
            );
        }

        info!(
            bond_balance = %bond_balance,
            wallet_balance = %wallet_balance,
            "Preconfer bonds are sufficient"
        );

        // Check ETH balance
        let balance = self
            .ethereum_l1
            .execution_layer
            .get_preconfer_eth_balance()
            .await
            .map_err(|e| Error::msg(format!("Failed to fetch ETH balance: {}", e)))?;

        if balance < self.thresholds.eth {
            anyhow::bail!(
                "ETH balance ({}) is below the required threshold ({})",
                balance,
                self.thresholds.eth
            );
        }

        info!("ETH balance is sufficient ({})", balance);

        // Wait for Taiko Geth to synchronize with L1
        let (mut taiko_inbox_height, mut taiko_geth_height) =
            self.get_current_protocol_height().await?;

        info!("Taiko Inbox Height: {taiko_inbox_height}, Taiko Geth Height: {taiko_geth_height}");

        while taiko_geth_height < taiko_inbox_height {
            warn!("Taiko Geth is behind L1. Waiting 5 seconds...");
            sleep(Duration::from_secs(5)).await;

            (taiko_inbox_height, taiko_geth_height) = self.get_current_protocol_height().await?;

            info!(
                "Taiko Inbox Height: {taiko_inbox_height}, Taiko Geth Height: {taiko_geth_height}"
            );
        }

        let (current_status, _) = self.operator.get_status().await?;

        // TODO check that when we are Preconfer or PreconferHandoverBuffer we will sync our l2 state on epoch boundry
        if matches!(
            current_status,
            OperatorStatus::None
                | OperatorStatus::Preconfer
                | OperatorStatus::PreconferHandoverBuffer
        ) {
            info!("Status: {:?}, no need for warmup", current_status);
            return Ok(());
        }

        if taiko_inbox_height == taiko_geth_height {
            return Ok(());
        } else {
            // We check previously that taiko_inbox_height > taiko_geth_height is not true,
            // so it is taiko_inbox_height < height_taiko_geth.
            // We have unprocessed L2 blocks.
            // Check if there is a pending tx on the mempool from your address.
            let nonce_latest: u64 = self
                .ethereum_l1
                .execution_layer
                .get_preconfer_nonce_latest()
                .await?;
            let nonce_pending: u64 = self
                .ethereum_l1
                .execution_layer
                .get_preconfer_nonce_pending()
                .await?;
            info!("Nonce Latest: {nonce_latest}, Nonce Pending: {nonce_pending}");
            //if not, then read blocks from L2 execution to form your buffer (L2 batch) and continue operations normally
            // we didn't propose blocks to mempool
            if nonce_latest == nonce_pending {
                // The first block anchor id is valid, so we can continue.
                if self
                    .batch_manager
                    .is_block_valid(taiko_inbox_height + 1)
                    .await?
                {
                    // recover all missed l2 blocks
                    info!("Recovering from L2 blocks");
                    for current_height in taiko_inbox_height + 1..=taiko_geth_height {
                        self.batch_manager
                            .recover_from_l2_block(current_height)
                            .await?;
                    }
                    // TODO calculate batch params and decide is it possible to continue with it, be careful with timeShift
                    // Sould be fixed with https://github.com/NethermindEth/Taiko-Preconf-AVS/issues/303
                    // Now just submit all the batches
                    self.batch_manager.try_submit_batches(false).await?;
                } else {
                    // The first block anchor id is not valid
                    // TODO reorg + reanchor + preconfirm again
                    // Just do force reorg
                    info!("Triggering L2 reorg");
                    self.batch_manager
                        .taiko
                        .trigger_l2_reorg(taiko_inbox_height)
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
                info!(
                    "Sleeping for {} ms to synchronize with L1 slot start",
                    duration.as_millis()
                );
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
                if let Err(err) = self.batch_manager.try_submit_batches(false).await {
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
        let l2_slot_info = self.batch_manager.taiko.get_l2_slot_info().await?;
        let (current_status, exit_point) = self.operator.get_status().await?;
        let pending_tx_list = self
            .batch_manager
            .taiko
            .get_pending_l2_tx_list_from_taiko_geth(l2_slot_info.base_fee())
            .await?;
        self.print_current_slots_info(
            &current_status,
            &pending_tx_list,
            &exit_point,
            l2_slot_info.base_fee(),
        )?;

        match current_status {
            OperatorStatus::PreconferHandoverBuffer => {
                // skip the slot
                return Ok(());
            }
            OperatorStatus::Preconfer => {
                self.preconfirm_block(false, pending_tx_list, l2_slot_info)
                    .await?;
            }
            OperatorStatus::PreconferAndL1Submitter => {
                self.preconfirm_block(true, pending_tx_list, l2_slot_info)
                    .await?;
            }
            OperatorStatus::L1Submitter => {
                self.batch_manager.try_submit_batches(false).await?;
            }
            OperatorStatus::None => {
                if self.batch_manager.has_batches() {
                    // TODO: Handle this situation gracefully
                    self.batch_manager.reset_builder();
                    warn!("Some batches were not successfully sent in the submitter window. Resetting batch builder.");
                }
            }
        }

        Ok(())
    }

    async fn preconfirm_block(
        &mut self,
        submit: bool,
        pending_tx_list: Option<PreBuiltTxList>,
        l2_slot_info: L2SlotInfo,
    ) -> Result<(), Error> {
        trace!("preconfirm_block: {submit} ");

        self.batch_manager
            .preconfirm_block(submit, pending_tx_list, l2_slot_info)
            .await
    }

    fn print_current_slots_info(
        &self,
        current_status: &OperatorStatus,
        pending_tx_list: &Option<PreBuiltTxList>,
        exit_point: &str,
        base_fee: u64,
    ) -> Result<(), Error> {
        let l1_slot = self.ethereum_l1.slot_clock.get_current_slot()?;
        info!(
            "| Epoch: {:<6} | Slot: {:<2} | L2 Slot: {:<2} | Pending txs: {:<4} | b. fee: {:<7} | {current_status} | {exit_point}",
            self.ethereum_l1.slot_clock.get_epoch_from_slot(l1_slot),
            self.ethereum_l1.slot_clock.slot_of_epoch(l1_slot),
            self.ethereum_l1
                .slot_clock
                .get_current_l2_slot_within_l1_slot()?,
            pending_tx_list
                .as_ref()
                .map_or(0, |tx_list| tx_list.tx_list.len()),
            base_fee
        );
        Ok(())
    }
}
