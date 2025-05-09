pub(crate) mod batch_manager;
mod operator;
mod verifier;

use crate::node::verifier::Verifier;
use crate::{
    ethereum_l1::{transaction_error::TransactionError, EthereumL1},
    metrics::Metrics,
    shared::{l2_slot_info::L2SlotInfo, l2_tx_lists::PreBuiltTxList},
    taiko::Taiko,
};
use alloy::primitives::U256;
use anyhow::Error;
use batch_manager::{BatchBuilderConfig, BatchManager};
use operator::{Operator, Status as OperatorStatus};
use std::sync::Arc;
use tokio::{
    sync::mpsc::{error::TryRecvError, Receiver},
    time::{sleep, Duration},
};
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
    verifier: Option<verifier::Verifier>,
    taiko: Arc<Taiko>,
    transaction_error_channel: Receiver<TransactionError>,
    metrics: Arc<Metrics>,
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
        simulate_not_submitting_at_the_end_of_epoch: bool,
        transaction_error_channel: Receiver<TransactionError>,
        metrics: Arc<Metrics>,
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
            &ethereum_l1,
            taiko.clone(),
            handover_window_slots,
            handover_start_buffer_ms,
            simulate_not_submitting_at_the_end_of_epoch,
        )?;
        let batch_manager = BatchManager::new(
            l1_height_lag,
            batch_builder_config,
            ethereum_l1.clone(),
            taiko.clone(),
        );
        Ok(Self {
            cancel_token,
            batch_manager: batch_manager,
            ethereum_l1,
            preconf_heartbeat_ms,
            operator,
            thresholds,
            verifier: None,
            taiko,
            transaction_error_channel,
            metrics,
        })
    }

    pub async fn entrypoint(mut self) -> Result<(), Error> {
        info!("Starting node");

        if let Err(err) = self.warmup().await {
            error!("Failed to warm up node: {}", err);
            return Err(anyhow::anyhow!(err));
        }

        info!("Node warmup successful");

        // Run preconfirmation loop in background
        tokio::spawn(async move {
            self.preconfirmation_loop().await;
        });

        Ok(())
    }

    async fn get_current_protocol_height(&self) -> Result<(u64, u64), Error> {
        let taiko_inbox_height = self
            .ethereum_l1
            .execution_layer
            .get_l2_height_from_taiko_inbox()
            .await?;

        let taiko_geth_height = self.taiko.get_latest_l2_block_id().await?;

        Ok((taiko_inbox_height, taiko_geth_height))
    }

    async fn warmup(&mut self) -> Result<(), Error> {
        info!("Warmup node");

        // Check TAIKO TOKEN balance
        let total_balance = self
            .ethereum_l1
            .execution_layer
            .get_preconfer_total_bonds()
            .await
            .map_err(|e| Error::msg(format!("Failed to fetch bond balance: {}", e)))?;

        if total_balance < self.thresholds.taiko {
            anyhow::bail!(
                "Total balance ({}) is below the required threshold ({})",
                total_balance,
                self.thresholds.taiko
            );
        }

        info!("Preconfer taiko balance are sufficient: {}", total_balance);

        // Check ETH balance
        let balance = self
            .ethereum_l1
            .execution_layer
            .get_preconfer_wallet_eth()
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

        // Wait for Taiko Driver to synchronize with Taiko Geth
        #[cfg(feature = "sync-on-warmup")]
        self.wait_for_taiko_driver_sync_with_geth().await;

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
                if let Err(err) = self.batch_manager.try_submit_oldest_batch(false).await {
                    error!("Failed to submit batches at the application shut down: {err}");
                }
                return;
            }

            if let Err(err) = self.main_block_preconfirmation_step().await {
                error!("Failed to execute main block preconfirmation step: {}", err);
            }
        }
    }

    async fn check_for_missing_proposed_batches(&mut self) -> Result<(), Error> {
        let (taiko_inbox_height, taiko_geth_height) = self.get_current_protocol_height().await?;

        info!(
            "📨 Taiko Inbox Height: {taiko_inbox_height}, Taiko Geth Height: {taiko_geth_height}"
        );

        if taiko_inbox_height == taiko_geth_height {
            return Ok(());
        } else {
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
            debug!("Nonce Latest: {nonce_latest}, Nonce Pending: {nonce_pending}");
            // TODO handle when not equal
            if nonce_latest == nonce_pending {
                // Just create a new verifier, we will check it in preconfirmation loop
                self.verifier = Some(
                    verifier::Verifier::new_with_taiko_height(
                        taiko_geth_height,
                        self.taiko.clone(),
                        self.batch_manager.clone_without_batches(),
                        0,
                    )
                    .await?,
                );
            }
        }

        Ok(())
    }

    /// Wait for Taiko Driver to synchronize with Taiko Geth chain tip.
    #[cfg(feature = "sync-on-warmup")]
    async fn wait_for_taiko_driver_sync_with_geth(&self) {
        // TODO move to config
        const TAIKO_DRIVER_SYNC_RETRY_PERIOD_BEFORE_PANIC_SEC: u64 = 600; // 10 mins

        let sleep_duration = Duration::from_millis(self.preconf_heartbeat_ms / 2);
        let start_time = std::time::SystemTime::now();
        while self
            .get_last_synced_block_height_between_taiko_geth_and_the_driver()
            .await
            .is_none()
        {
            if let Ok(elapsed) = start_time.elapsed() {
                if elapsed > Duration::from_secs(TAIKO_DRIVER_SYNC_RETRY_PERIOD_BEFORE_PANIC_SEC) {
                    error!(
                        "Driver sync exceeded max retry period before panic {}. Shutting down...",
                        TAIKO_DRIVER_SYNC_RETRY_PERIOD_BEFORE_PANIC_SEC
                    );
                    self.cancel_token.cancel();
                }
            }
            sleep(sleep_duration).await;
        }

        if let Ok(elapsed) = start_time.elapsed() {
            warn!("⭕ Driver sync took: {} ms.", elapsed.as_millis());
        }
    }

    async fn get_last_synced_block_height_between_taiko_geth_and_the_driver(&self) -> Option<u64> {
        if let Ok(taiko_geth_height) = self.taiko.get_latest_l2_block_id().await {
            match self.taiko.get_status().await {
                Ok(status) => {
                    info!(
                        "🌀 Taiko status highestUnsafeL2PayloadBlockID: {}, Taiko Geth Height: {}",
                        status.highest_unsafe_l2_payload_block_id, taiko_geth_height
                    );
                    if taiko_geth_height == status.highest_unsafe_l2_payload_block_id {
                        return Some(taiko_geth_height);
                    }
                }
                Err(err) => {
                    error!("Failed to get status from taiko driver: {}", err);
                }
            }
        }
        None
    }

    async fn main_block_preconfirmation_step(&mut self) -> Result<(), Error> {
        let l2_slot_info = self.taiko.get_l2_slot_info().await?;
        let current_status = self.operator.get_status(&l2_slot_info).await?;
        let pending_tx_list = self
            .batch_manager
            .taiko
            .get_pending_l2_tx_list_from_taiko_geth(l2_slot_info.base_fee())
            .await?;
        self.print_current_slots_info(
            &current_status,
            &pending_tx_list,
            l2_slot_info.base_fee(),
            self.batch_manager.get_number_of_batches(),
        )?;

        self.handle_transaction_error(&current_status).await?;

        if current_status.is_preconfirmation_start_slot() {
            if current_status.is_submitter() {
                // We start preconfirmation in the middle of the epoch.
                // Need to check for unproposed L2 blocks.
                if let Err(err) = self.check_for_missing_proposed_batches().await {
                    error!(
                        "Shutdown: Failed to verify proposed batches on startup: {}",
                        err
                    );
                    self.cancel_token.cancel();
                    return Err(anyhow::anyhow!(
                        "Shutdown: Failed to verify proposed batches on startup: {}",
                        err
                    ));
                }
            } else {
                // It is for handover window
                if let Some(taiko_geth_height) = self
                    .get_last_synced_block_height_between_taiko_geth_and_the_driver()
                    .await
                {
                    let verification_slot =
                        self.ethereum_l1.slot_clock.get_next_epoch_start_slot()?;
                    let verifier_result = verifier::Verifier::new_with_taiko_height(
                        taiko_geth_height,
                        self.taiko.clone(),
                        self.batch_manager.clone_without_batches(),
                        verification_slot,
                    )
                    .await;
                    match verifier_result {
                        Ok(verifier) => {
                            self.verifier = Some(verifier);
                        }
                        Err(err) => {
                            error!("Shutdown: Failed to create verifier: {}", err);
                            self.cancel_token.cancel();
                            return Err(anyhow::anyhow!(
                                "Shutdown: Failed to create verifier on startup: {}",
                                err
                            ));
                        }
                    }
                } else {
                    // skip slot driver is not synced with geth
                    return Ok(());
                }
            }
        }

        if current_status.is_preconfer() {
            self.preconfirm_block(
                pending_tx_list,
                l2_slot_info,
                current_status.is_end_of_sequencing(),
            )
            .await?;
        }

        if current_status.is_submitter() {
            // first submit verification batches
            if let Some(mut verifier) = self.verifier.take() {
                match self
                    .verify_proposed_but_not_submitted_batches(&mut verifier)
                    .await
                {
                    Ok(success) => {
                        if !success {
                            self.verifier = Some(verifier);
                            return Ok(());
                        }
                    }
                    Err(err) => {
                        self.verifier = Some(verifier);
                        return Err(err);
                    }
                };
            } else {
                self.batch_manager
                    .try_submit_oldest_batch(current_status.is_preconfer())
                    .await?;
            }
        }

        if !current_status.is_submitter() && !current_status.is_preconfer() {
            if self.batch_manager.has_batches() {
                self.batch_manager.reset_builder();
                error!("Some batches were not successfully sent in the submitter window. Resetting batch builder.");
            }
            if self.verifier.is_some() {
                error!("Verifier is not None after submitter window.");
                self.verifier = None;
            }
        }

        Ok(())
    }

    /// Returns true if the operation succeeds
    async fn verify_proposed_but_not_submitted_batches(
        &mut self,
        verifier: &mut Verifier,
    ) -> Result<bool, Error> {
        if verifier.has_blocks_to_verify() {
            let head_slot = self
                .ethereum_l1
                ._consensus_layer
                .get_head_slot_number()
                .await?;

            if !verifier.is_slot_valid(head_slot) {
                info!(
                    "Slot {} is not valid for verification, target slot {}, skipping",
                    head_slot,
                    verifier.get_verification_slot()
                );
                return Ok(false);
            }

            let taiko_inbox_height = self
                .ethereum_l1
                .execution_layer
                .get_l2_height_from_taiko_inbox()
                .await?;
            if let Err(err) = verifier
                .verify_submitted_blocks(taiko_inbox_height, self.metrics.clone())
                .await
            {
                self.trigger_l2_reorg(
                    taiko_inbox_height,
                    &format!("Verifier return an error: {}", err),
                )
                .await?;
                return Ok(true);
            }
        }

        if verifier.has_batches_to_submit() {
            verifier.try_submit_oldest_batch().await?;
        }

        return Ok(!verifier.has_batches_to_submit());
    }

    async fn handle_transaction_error(
        &mut self,
        current_status: &OperatorStatus,
    ) -> Result<(), Error> {
        match self.transaction_error_channel.try_recv() {
            Ok(error) => match error {
                TransactionError::TransactionReverted => {
                    if current_status.is_preconfer() && current_status.is_submitter() {
                        let taiko_inbox_height = self
                            .ethereum_l1
                            .execution_layer
                            .get_l2_height_from_taiko_inbox()
                            .await?;
                        if let Err(err) = self
                            .trigger_l2_reorg(taiko_inbox_height, "Transaction reverted")
                            .await
                        {
                            self.cancel_token.cancel();
                            return Err(anyhow::anyhow!("Failed to trigger L2 reorg: {}", err));
                        }
                    } else {
                        warn!("Transaction reverted, not our epoch, skipping reorg");
                    }
                }
                TransactionError::NotConfirmed => {
                    self.cancel_token.cancel();
                    return Err(anyhow::anyhow!(
                        "Transaction not confirmed for a long time, exiting"
                    ));
                }
                TransactionError::UnsupportedTransactionType => {
                    self.cancel_token.cancel();
                    return Err(anyhow::anyhow!(
                        "Unsupported transaction type. You can send eip1559 or eip4844 transactions only"
                    ));
                }
                TransactionError::GetBlockNumberFailed => {
                    // TODO recreate L1 provider
                    self.cancel_token.cancel();
                    return Err(anyhow::anyhow!("Failed to get block number from L1"));
                }
            },
            Err(err) => match err {
                TryRecvError::Empty => {
                    // no errors, proceed with preconfirmation
                }
                TryRecvError::Disconnected => {
                    self.cancel_token.cancel();
                    return Err(anyhow::anyhow!("Transaction error channel disconnected"));
                }
            },
        }

        Ok(())
    }

    async fn preconfirm_block(
        &mut self,
        pending_tx_list: Option<PreBuiltTxList>,
        l2_slot_info: L2SlotInfo,
        end_of_sequencing: bool,
    ) -> Result<(), Error> {
        trace!("preconfirm_block");

        self.batch_manager
            .preconfirm_block(pending_tx_list, l2_slot_info, end_of_sequencing)
            .await
    }

    fn print_current_slots_info(
        &self,
        current_status: &OperatorStatus,
        pending_tx_list: &Option<PreBuiltTxList>,
        base_fee: u64,
        batches_number: u64,
    ) -> Result<(), Error> {
        let l1_slot = self.ethereum_l1.slot_clock.get_current_slot()?;
        info!(
            "| Epoch: {:<6} | Slot: {:<2} | L2 Slot: {:<2} | Pending txs: {:<4} | b. fee: {:<7} | Batches: {batches_number} | {current_status} |",
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

    async fn trigger_l2_reorg(
        &mut self,
        new_last_block_id: u64,
        message: &str,
    ) -> Result<(), Error> {
        warn!("⛓️‍💥 Force Reorg: {}", message);
        self.batch_manager
            .trigger_l2_reorg(new_last_block_id)
            .await?;
        self.verifier = None;
        Ok(())
    }
}
