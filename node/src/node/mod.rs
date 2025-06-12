pub(crate) mod batch_manager;
pub mod blob_parser;
mod l2_head_verifier;
mod operator;
mod verifier;

use crate::chain_monitor;
use crate::forced_inclusion_monitor::ForcedInclusionMonitor;
use crate::{
    ethereum_l1::{EthereumL1, transaction_error::TransactionError},
    metrics::Metrics,
    node::l2_head_verifier::L2HeadVerifier,
    shared::{l2_slot_info::L2SlotInfo, l2_tx_lists::PreBuiltTxList},
    taiko::{Taiko, preconf_blocks::BuildPreconfBlockResponse},
};
use alloy::primitives::U256;
use anyhow::Error;
use batch_manager::{BatchBuilderConfig, BatchManager};
use chain_monitor::ChainMonitor;
use operator::{Operator, Status as OperatorStatus};
use std::sync::Arc;
use tokio::{
    sync::mpsc::{Receiver, error::TryRecvError},
    time::{Duration, sleep},
};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};
use verifier::VerificationResult;

pub struct Thresholds {
    pub eth: U256,
    pub taiko: U256,
}

pub struct Node {
    cancel_token: CancellationToken,
    ethereum_l1: Arc<EthereumL1>,
    chain_monitor: Arc<ChainMonitor>,
    preconf_heartbeat_ms: u64,
    operator: Operator,
    batch_manager: BatchManager,
    thresholds: Thresholds,
    verifier: Option<verifier::Verifier>,
    taiko: Arc<Taiko>,
    transaction_error_channel: Receiver<TransactionError>,
    metrics: Arc<Metrics>,
    watchdog: u64,
    head_verifier: L2HeadVerifier,
}

impl Node {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        cancel_token: CancellationToken,
        taiko: Arc<Taiko>,
        ethereum_l1: Arc<EthereumL1>,
        chain_monitor: Arc<ChainMonitor>,
        preconf_heartbeat_ms: u64,
        handover_window_slots: u64,
        handover_start_buffer_ms: u64,
        l1_height_lag: u64,
        batch_builder_config: BatchBuilderConfig,
        thresholds: Thresholds,
        simulate_not_submitting_at_the_end_of_epoch: bool,
        transaction_error_channel: Receiver<TransactionError>,
        metrics: Arc<Metrics>,
        forced_inclusion_monitor: Arc<ForcedInclusionMonitor>,
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
            cancel_token.clone(),
        )?;
        let batch_manager = BatchManager::new(
            l1_height_lag,
            batch_builder_config,
            ethereum_l1.clone(),
            taiko.clone(),
            forced_inclusion_monitor,
        );
        let head_verifier = L2HeadVerifier::new();
        Ok(Self {
            cancel_token,
            batch_manager,
            ethereum_l1,
            chain_monitor,
            preconf_heartbeat_ms,
            operator,
            thresholds,
            verifier: None,
            taiko,
            transaction_error_channel,
            metrics,
            watchdog: 0,
            head_verifier,
        })
    }

    pub async fn entrypoint(mut self) -> Result<(), Error> {
        info!("Starting node");

        if let Err(err) = self.warmup().await {
            error!("Failed to warm up node: {}. Shutting down.", err);
            self.cancel_token.cancel();
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

        // Wait for the last sent transaction to be executed
        self.wait_for_sent_transactions().await?;

        Ok(())
    }

    async fn wait_for_sent_transactions(&self) -> Result<(), Error> {
        loop {
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
            if nonce_pending == nonce_latest {
                break;
            }
            debug!(
                "Waiting for sent transactions to be executed. Nonce Latest: {nonce_latest}, Nonce Pending: {nonce_pending}"
            );
            sleep(Duration::from_secs(6)).await;
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
                return;
            }

            if let Err(err) = self.main_block_preconfirmation_step().await {
                self.watchdog += 1;
                error!("Failed to execute main block preconfirmation step: {}", err);
                if self.watchdog > self.ethereum_l1.slot_clock.get_l2_slots_per_epoch() / 2 {
                    error!(
                        "Watchdog triggered after {} heartbeats, shutting down...",
                        self.watchdog
                    );
                    self.cancel_token.cancel();
                    return;
                }
            } else {
                self.watchdog = 0;
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
                        self.cancel_token.clone(),
                    )
                    .await?,
                );
            }
        }

        Ok(())
    }

    async fn main_block_preconfirmation_step(&mut self) -> Result<(), Error> {
        let (l2_slot_info, current_status, pending_tx_list) =
            self.get_slot_info_and_status().await?;

        // Get the transaction status before checking the error channel
        // to avoid race condition
        let transaction_in_progress = self
            .ethereum_l1
            .execution_layer
            .is_transaction_in_progress()
            .await?;

        self.check_transaction_error_channel(&current_status)
            .await?;

        if current_status.is_preconfirmation_start_slot() {
            self.head_verifier
                .set(l2_slot_info.parent_id(), *l2_slot_info.parent_hash())
                .await;

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
                let taiko_geth_height = l2_slot_info.parent_id();
                let verification_slot = self.ethereum_l1.slot_clock.get_next_epoch_start_slot()?;
                let verifier_result = verifier::Verifier::new_with_taiko_height(
                    taiko_geth_height,
                    self.taiko.clone(),
                    self.batch_manager.clone_without_batches(),
                    verification_slot,
                    self.cancel_token.clone(),
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
            }
        }

        if current_status.is_preconfer() && current_status.is_driver_synced() {
            // do not trigger fast reanchor on submitter window to prevent from double reanchor
            if !current_status.is_submitter()
                && self
                    .check_and_handle_anchor_offset_for_unsafe_l2_blocks(&l2_slot_info)
                    .await?
            {
                // reanchored, no need to preconf
                return Ok(());
            }

            if !self
                .head_verifier
                .verify(l2_slot_info.parent_id(), l2_slot_info.parent_hash())
                .await
            {
                self.head_verifier.log_error().await;
                self.cancel_token.cancel();
                return Err(anyhow::anyhow!(
                    "Unexpected L2 head detected. Restarting node..."
                ));
            }
            if let Some(block) = self
                .preconfirm_block(
                    pending_tx_list,
                    l2_slot_info,
                    current_status.is_end_of_sequencing(),
                    current_status.is_submitter() && self.verifier.is_none(),
                )
                .await?
            {
                if !self
                    .head_verifier
                    .verify_next_and_set(block.number, block.hash, block.parent_hash)
                    .await
                {
                    self.head_verifier.log_error().await;
                    self.cancel_token.cancel();
                    return Err(anyhow::anyhow!(
                        "Unexpected L2 head after preconfirmation. Restarting node..."
                    ));
                }
            }
        }

        if current_status.is_submitter() && !transaction_in_progress {
            // first check verifier
            if self.has_verified_unproposed_batches().await? {
                if let Err(err) = self
                    .batch_manager
                    .try_submit_oldest_batch(current_status.is_preconfer())
                    .await
                {
                    if let Some(transaction_error) = err.downcast_ref::<TransactionError>() {
                        self.handle_transaction_error(transaction_error, &current_status)
                            .await?;
                    }
                    return Err(err);
                }
            }
        }

        if !current_status.is_submitter() && !current_status.is_preconfer() {
            if self.batch_manager.has_batches() {
                self.batch_manager.reset_builder();
                error!(
                    "Some batches were not successfully sent in the submitter window. Resetting batch builder."
                );
            }
            if self.verifier.is_some() {
                error!("Verifier is not None after submitter window.");
                self.verifier = None;
            }
        }

        Ok(())
    }

    /// Checks the anchor offset for unsafe L2 blocks and triggers a reanchor if necessary.
    /// Returns true if reanchor was triggered.
    async fn check_and_handle_anchor_offset_for_unsafe_l2_blocks(
        &mut self,
        l2_slot_info: &L2SlotInfo,
    ) -> Result<bool, Error> {
        debug!("Checking anchor offset for unsafe L2 blocks to do fast reanchor when needed");
        let taiko_inbox_height = self
            .ethereum_l1
            .execution_layer
            .get_l2_height_from_taiko_inbox()
            .await?;
        if taiko_inbox_height < l2_slot_info.parent_id() {
            let l2_block_id = taiko_inbox_height + 1;
            let anchor_offset = self
                .batch_manager
                .get_l1_anchor_block_offset_for_l2_block(l2_block_id)
                .await?;
            let max_anchor_height_offset = self
                .ethereum_l1
                .execution_layer
                .get_config_max_anchor_height_offset();

            // +1 because we are checking the next block
            if anchor_offset > max_anchor_height_offset + 1 {
                warn!(
                    "Anchor offset {} is too high for l2 block id {}, triggering reanchor",
                    anchor_offset, l2_block_id
                );
                if let Err(err) = self
                    .reanchor_blocks(
                        taiko_inbox_height,
                        "Anchor offset is too high for unsafe L2 blocks",
                    )
                    .await
                {
                    error!("Failed to reanchor: {}", err);
                    self.cancel_token.cancel();
                    return Err(anyhow::anyhow!("Failed to reanchor: {}", err));
                }
                return Ok(true);
            }
        }

        Ok(false)
    }

    async fn get_slot_info_and_status(
        &mut self,
    ) -> Result<(L2SlotInfo, OperatorStatus, Option<PreBuiltTxList>), Error> {
        let l2_slot_info = self.taiko.get_l2_slot_info().await;
        let current_status = match &l2_slot_info {
            Ok(info) => self.operator.get_status(info).await,
            Err(_) => Err(anyhow::anyhow!("Failed to get L2 slot info")),
        };
        let batches_ready_to_send = self.batch_manager.get_number_of_batches_ready_to_send();
        let pending_tx_list = match &l2_slot_info {
            Ok(info) => {
                self.batch_manager
                    .taiko
                    .get_pending_l2_tx_list_from_taiko_geth(info.base_fee(), batches_ready_to_send)
                    .await
            }
            Err(_) => Err(anyhow::anyhow!("Failed to get L2 slot info")),
        };
        self.print_current_slots_info(
            &current_status,
            &pending_tx_list,
            &l2_slot_info,
            self.batch_manager.get_number_of_batches(),
        )?;

        Ok((l2_slot_info?, current_status?, pending_tx_list?))
    }

    /// Returns true if the operation succeeds
    async fn has_verified_unproposed_batches(&mut self) -> Result<bool, Error> {
        if let Some(mut verifier) = self.verifier.take() {
            match verifier
                .verify(self.ethereum_l1.clone(), self.metrics.clone())
                .await
            {
                Ok(res) => match res {
                    VerificationResult::SlotNotValid => {
                        self.verifier = Some(verifier);
                        return Ok(false);
                    }
                    VerificationResult::ReanchorNeeded(block, reason) => {
                        if let Err(err) = self.reanchor_blocks(block, &reason).await {
                            error!("Failed to reanchor blocks: {}", err);
                            self.cancel_token.cancel();
                            return Err(err);
                        }
                    }
                    VerificationResult::SuccessWithBatches(batches) => {
                        self.batch_manager.prepend_batches(batches);
                    }
                    VerificationResult::SuccessNoBatches => {}
                    VerificationResult::VerificationInProgress => {
                        self.verifier = Some(verifier);
                        return Ok(false);
                    }
                },
                Err(err) => {
                    self.verifier = Some(verifier);
                    return Err(err);
                }
            }
        }
        Ok(true)
    }

    async fn check_transaction_error_channel(
        &mut self,
        current_status: &OperatorStatus,
    ) -> Result<(), Error> {
        match self.transaction_error_channel.try_recv() {
            Ok(error) => {
                return self.handle_transaction_error(&error, current_status).await;
            }
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

    async fn handle_transaction_error(
        &mut self,
        error: &TransactionError,
        current_status: &OperatorStatus,
    ) -> Result<(), Error> {
        match error {
            TransactionError::ReanchorRequired => {
                if current_status.is_preconfer() && current_status.is_submitter() {
                    let taiko_inbox_height = self
                        .ethereum_l1
                        .execution_layer
                        .get_l2_height_from_taiko_inbox()
                        .await?;
                    if let Err(err) = self
                        .reanchor_blocks(taiko_inbox_height, "Transaction reverted")
                        .await
                    {
                        error!("Failed to reanchor blocks: {}", err);
                        self.cancel_token.cancel();
                        return Err(anyhow::anyhow!("Failed to reanchor blocks: {}", err));
                    }
                    return Err(anyhow::anyhow!("Reanchoring done"));
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
            TransactionError::EstimationTooEarly => {
                return Err(anyhow::anyhow!(
                    "Transaction estimation too early, skipping slot"
                ));
            }
            TransactionError::TimestampTooLarge => {
                self.cancel_token.cancel();
                return Err(anyhow::anyhow!(
                    "Transaction reverted with TimestampTooLarge error"
                ));
            }
            TransactionError::InsufficientFunds => {
                self.cancel_token.cancel();
                return Err(anyhow::anyhow!(
                    "Transaction reverted with InsufficientFunds error"
                ));
            }
            TransactionError::EstimationFailed => {
                self.cancel_token.cancel();
                return Err(anyhow::anyhow!("Transaction estimation failed, exiting"));
            }
            TransactionError::TransactionReverted => {
                self.cancel_token.cancel();
                return Err(anyhow::anyhow!("Transaction reverted, exiting"));
            }
        }

        Ok(())
    }

    async fn preconfirm_block(
        &mut self,
        pending_tx_list: Option<PreBuiltTxList>,
        l2_slot_info: L2SlotInfo,
        end_of_sequencing: bool,
        can_do_forced_inclusion: bool,
    ) -> Result<Option<BuildPreconfBlockResponse>, Error> {
        self.batch_manager
            .preconfirm_block(
                pending_tx_list,
                l2_slot_info,
                end_of_sequencing,
                can_do_forced_inclusion,
            )
            .await
    }

    fn print_current_slots_info(
        &self,
        current_status: &Result<OperatorStatus, Error>,
        pending_tx_list: &Result<Option<PreBuiltTxList>, Error>,
        l2_slot_info: &Result<L2SlotInfo, Error>,
        batches_number: u64,
    ) -> Result<(), Error> {
        let l1_slot = self.ethereum_l1.slot_clock.get_current_slot()?;
        info!(target: "heartbeat",
            "| Epoch: {:<6} | Slot: {:<2} | L2 Slot: {:<2} | {}{} Batches: {batches_number} | {} |",
            self.ethereum_l1.slot_clock.get_epoch_from_slot(l1_slot),
            self.ethereum_l1.slot_clock.slot_of_epoch(l1_slot),
            self.ethereum_l1
                .slot_clock
                .get_current_l2_slot_within_l1_slot()?,
            if let Ok(pending_tx_list) = pending_tx_list {
                format!(
                    "Txs: {:<4} |",
                    pending_tx_list
                        .as_ref()
                        .map_or(0, |tx_list| tx_list.tx_list.len())
                )
            } else {
                "Txs: unknown |".to_string()
            },
            if let Ok(l2_slot_info) = l2_slot_info {
                format!(
                    " Fee: {:<7} | L2: {:<6} | Time: {:<10} | Hash: {} |",
                    l2_slot_info.base_fee(),
                    l2_slot_info.parent_id(),
                    l2_slot_info.slot_timestamp(),
                    &l2_slot_info.parent_hash().to_string()[..8]
                )
            } else {
                " L2 slot info unknown |".to_string()
            },
            if let Ok(status) = current_status {
                status.to_string()
            } else {
                "Unknown".to_string()
            },
        );
        Ok(())
    }

    async fn reanchor_blocks(&mut self, parent_block_id: u64, reason: &str) -> Result<(), Error> {
        warn!(
            "⛓️‍💥 Reanchoring blocks for parent block: {} reason: {}",
            parent_block_id, reason
        );

        let start_time = std::time::Instant::now();

        let mut l2_slot_info = self
            .taiko
            .get_l2_slot_info_by_parent_block(alloy::eips::BlockNumberOrTag::Number(
                parent_block_id,
            ))
            .await?;

        // Update self state
        self.verifier = None;
        self.batch_manager.reset_builder();

        self.chain_monitor.set_expected_reorg(parent_block_id).await;

        let start_block_id = parent_block_id + 1;
        let blocks = self
            .taiko
            .fetch_l2_blocks_until_latest(start_block_id, true)
            .await?;

        let blocks_reanchored = blocks.len() as u64;

        for block in blocks {
            debug!(
                "Reanchoring block {} with {} transactions, parent_id {}, parent_hash {}",
                block.header.number,
                block.transactions.len(),
                l2_slot_info.parent_id(),
                l2_slot_info.parent_hash(),
            );

            let (_, txs) = match block.transactions.as_transactions() {
                Some(txs) => txs.split_first().ok_or_else(|| {
                    anyhow::anyhow!(
                        "Cannot get anchor transaction from block {}",
                        block.header.number
                    )
                })?,
                None => {
                    return Err(anyhow::anyhow!(
                        "No transactions in block {}",
                        block.header.number
                    ));
                }
            };

            let tx_list = txs.to_vec();
            let bytes_length =
                crate::shared::l2_tx_lists::encode_and_compress(&tx_list)?.len() as u64;
            let pending_tx_list = crate::shared::l2_tx_lists::PreBuiltTxList {
                tx_list,
                estimated_gas_used: 0,
                bytes_length,
            };

            let block = self
                .batch_manager
                .reanchor_block(pending_tx_list, l2_slot_info)
                .await;
            // if reanchor_block fails restart the node
            if let Ok(Some(block)) = block {
                debug!("Reanchored block {} hash {}", block.number, block.hash);
            } else {
                let err_msg = match block {
                    Ok(None) => "Failed to reanchor block: None returned".to_string(),
                    Err(err) => format!("Failed to reanchor block: {}", err),
                    Ok(Some(_)) => "Unreachable".to_string(),
                };
                error!("{}", err_msg);
                self.cancel_token.cancel();
                return Err(anyhow::anyhow!("{}", err_msg));
            }

            // TODO reduce 1 geth call
            // We can get previous L2 slot info from BuildPreconfBlockResponse
            l2_slot_info = self.taiko.get_l2_slot_info().await?;
        }

        self.head_verifier
            .set(l2_slot_info.parent_id(), *l2_slot_info.parent_hash())
            .await;

        self.metrics.inc_by_blocks_reanchored(blocks_reanchored);

        debug!(
            "Finished reanchoring blocks for parent block {} in {} ms",
            parent_block_id,
            start_time.elapsed().as_millis()
        );
        Ok(())
    }
}
