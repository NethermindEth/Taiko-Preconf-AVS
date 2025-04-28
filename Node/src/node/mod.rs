pub(crate) mod batch_manager;
mod operator;
mod verifier;

use crate::{
    ethereum_l1::{transaction_error::TransactionError, EthereumL1},
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
    verifier: Box<verifier::Verifier>,
    taiko: Arc<Taiko>,
    transaction_error_channel: Receiver<TransactionError>,
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
        let verifier =
            verifier::Verifier::new(taiko.clone(), batch_manager.clone_without_batches()).await?;
        Ok(Self {
            cancel_token,
            batch_manager: batch_manager,
            ethereum_l1,
            preconf_heartbeat_ms,
            operator,
            thresholds,
            verifier: Box::new(verifier),
            taiko,
            transaction_error_channel,
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

    async fn verify_proposed_batches(&mut self) -> Result<(), Error> {
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
            if nonce_latest == nonce_pending {
                let mut verifier = verifier::Verifier::new_with_taiko_height(
                    taiko_geth_height,
                    self.taiko.clone(),
                    self.batch_manager.clone_without_batches(),
                );
                if let Err(e) = verifier.verify_submitted_blocks(taiko_inbox_height).await {
                    warn!("Force Reorg: Verifier return an error: {}", e);
                    self.batch_manager
                        .trigger_l2_reorg(taiko_inbox_height)
                        .await?;
                }
            }
        }

        Ok(())
    }

    async fn main_block_preconfirmation_step(&mut self) -> Result<(), Error> {
        let l2_slot_info = self.taiko.get_l2_slot_info().await?;
        let current_status = self.operator.get_status().await?;
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

        self.handle_transaction_error().await?;

        if current_status.is_preconfirmation_start_slot() {
            if current_status.is_submitter() {
                // We start preconfirmation in the middle of the epoch.
                // Need to check for unproposed L2 blocks.
                self.verify_proposed_batches().await?;
            } else {
                // Expected behaviour
                self.verifier = Box::new(
                    verifier::Verifier::new(
                        self.taiko.clone(),
                        self.batch_manager.clone_without_batches(),
                    )
                    .await?,
                );
            }
        }

        if current_status.is_preconfer() {
            self.preconfirm_block(pending_tx_list, l2_slot_info).await?;
        }

        if current_status.is_verifier() {
            let taiko_inbox_height = self
                .ethereum_l1
                .execution_layer
                .get_l2_height_from_taiko_inbox()
                .await?;
            if let Err(e) = self
                .verifier
                .verify_submitted_blocks(taiko_inbox_height)
                .await
            {
                warn!("Force Reorg: Verifier return an error: {}", e);
                self.batch_manager
                    .trigger_l2_reorg(taiko_inbox_height)
                    .await?;
            }
        }

        if current_status.is_submitter() {
            // first submit verification batches
            if self.verifier.has_batches_to_submit() {
                self.verifier.try_submit_oldest_batch().await?;
            } else {
                self.batch_manager
                    .try_submit_oldest_batch(current_status.is_preconfer())
                    .await?;
            }
        }

        if !current_status.is_submitter()
            && !current_status.is_preconfer()
            && self.batch_manager.has_batches()
        {
            self.batch_manager.reset_builder();
            error!("Some batches were not successfully sent in the submitter window. Resetting batch builder.");
        }

        Ok(())
    }

    async fn handle_transaction_error(&mut self) -> Result<(), Error> {
        match self.transaction_error_channel.try_recv() {
            Ok(error) => match error {
                TransactionError::TransactionReverted => {
                    let taiko_inbox_height = self
                        .ethereum_l1
                        .execution_layer
                        .get_l2_height_from_taiko_inbox()
                        .await?;
                    warn!("Force Reorg: Transaction reverted");
                    if let Err(e) = self
                        .batch_manager
                        .trigger_l2_reorg(taiko_inbox_height)
                        .await
                    {
                        self.cancel_token.cancel();
                        return Err(anyhow::anyhow!("Failed to trigger L2 reorg: {}", e));
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
    ) -> Result<(), Error> {
        trace!("preconfirm_block");

        self.batch_manager
            .preconfirm_block(pending_tx_list, l2_slot_info)
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
}
