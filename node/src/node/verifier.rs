use alloy::primitives::B256;
use anyhow::Error;
use std::{cmp::Ordering, collections::VecDeque, sync::Arc};
use tokio::{sync::Mutex, task::JoinHandle};
use tracing::{debug, info, warn};

use crate::{ethereum_l1::EthereumL1, taiko::Taiko, utils::types::Slot};

use super::batch_manager::{BatchManager, batch_builder::Batch};

use crate::Metrics;

pub enum VerificationResult {
    SuccessNoBatches,
    SuccessWithBatches(VecDeque<Batch>),
    ReanchorNeeded(u64, String),
    SlotNotValid,
    VerificationInProgress,
}

#[derive(Clone)]
struct PreconfirmationRootBlock {
    number: u64,
    hash: B256,
}

pub struct Verifier {
    verification_slot: Slot,
    verifier_thread: Option<VerifierThread>,
    verifier_thread_handle: Option<JoinHandle<Result<VecDeque<Batch>, Error>>>,
    preconfirmation_root: PreconfirmationRootBlock,
    thread_start_mutex: Mutex<()>,
}

struct VerifierThread {
    taiko: Arc<Taiko>,
    preconfirmation_root: PreconfirmationRootBlock,
    batch_manager: BatchManager,
}

impl Verifier {
    pub async fn new_with_taiko_height(
        taiko_geth_height: u64,
        taiko: Arc<Taiko>,
        batch_manager: BatchManager,
        verification_slot: Slot,
        //TODO: add cancel token
    ) -> Result<Self, Error> {
        let hash = taiko.get_l2_block_hash(taiko_geth_height).await?;
        debug!(
            "Verifier created with taiko_geth_height: {}, hash: {}, verification_slot: {}",
            taiko_geth_height, hash, verification_slot
        );
        let preconfirmation_root = PreconfirmationRootBlock {
            number: taiko_geth_height,
            hash,
        };
        Ok(Self {
            verifier_thread: Some(VerifierThread {
                taiko,
                preconfirmation_root: preconfirmation_root.clone(),
                batch_manager,
            }),
            verification_slot,
            verifier_thread_handle: None,
            preconfirmation_root,
            thread_start_mutex: Mutex::new(()),
        })
    }

    pub fn is_slot_valid(&self, current_slot: Slot) -> bool {
        current_slot >= self.verification_slot
    }

    pub fn get_verification_slot(&self) -> Slot {
        self.verification_slot
    }

    pub fn has_blocks_to_verify(&self) -> bool {
        self.preconfirmation_root.number > 0
    }

    async fn start_verification_thread(&mut self, taiko_inbox_height: u64, metrics: Arc<Metrics>) {
        let _guard = self.thread_start_mutex.lock().await; // protect from multiple threads
        if let Some(mut verifier_thread) = self.verifier_thread.take() {
            self.verifier_thread_handle = Some(tokio::spawn(async move {
                info!("🔍 Started block verification thread");
                verifier_thread
                    .verify_submitted_blocks(taiko_inbox_height, metrics)
                    .await
            }));
        } else {
            warn!("Verifier thread already started");
        }
    }

    /// Returns true if the operation succeeds
    pub async fn verify(
        &mut self,
        ethereum_l1: Arc<EthereumL1>,
        metrics: Arc<Metrics>,
    ) -> Result<VerificationResult, Error> {
        if let Some(handle) = self.verifier_thread_handle.as_mut() {
            if handle.is_finished() {
                let result = handle.await?;
                match result {
                    Ok(batches) => {
                        if batches.is_empty() {
                            return Ok(VerificationResult::SuccessNoBatches);
                        }
                        return Ok(VerificationResult::SuccessWithBatches(batches));
                    }
                    Err(err) => {
                        let taiko_inbox_height = ethereum_l1
                            .execution_layer
                            .get_l2_height_from_taiko_inbox()
                            .await?;
                        return Ok(VerificationResult::ReanchorNeeded(
                            taiko_inbox_height,
                            format!("Verifier return an error: {}", err),
                        ));
                    }
                }
            } else {
                return Ok(VerificationResult::VerificationInProgress);
            }
        } else {
            if self.has_blocks_to_verify() {
                let head_slot = ethereum_l1.consensus_layer.get_head_slot_number().await?;

                if !self.is_slot_valid(head_slot) {
                    info!(
                        "Slot {} is not valid for verification, target slot {}, skipping",
                        head_slot,
                        self.get_verification_slot()
                    );
                    return Ok(VerificationResult::SlotNotValid);
                }

                let taiko_inbox_height = ethereum_l1
                    .execution_layer
                    .get_l2_height_from_taiko_inbox()
                    .await?;
                self.start_verification_thread(taiko_inbox_height, metrics)
                    .await;

                return Ok(VerificationResult::VerificationInProgress);
            }
            return Ok(VerificationResult::SuccessNoBatches);
        }
    }
}

impl VerifierThread {
    async fn verify_submitted_blocks(
        &mut self,
        taiko_inbox_height: u64,
        metrics: Arc<Metrics>,
    ) -> Result<VecDeque<Batch>, Error> {
        // Compare block hashes to confirm that the block is still the same.
        // If not, return an error that will trigger a reorg.
        let current_hash = self
            .taiko
            .get_l2_block_hash(self.preconfirmation_root.number)
            .await?;
        if self.preconfirmation_root.hash != current_hash {
            return Err(anyhow::anyhow!(
                "❌ Block {} hash mismatch: current: {}, expected: {}",
                self.preconfirmation_root.number,
                current_hash,
                self.preconfirmation_root.hash
            ));
        }

        match self.preconfirmation_root.number.cmp(&taiko_inbox_height) {
            Ordering::Greater => {
                // preconfirmation_root.number > taiko_inbox_height
                // make batches from blocks unprocessed by previous preconfer
                info!(
                    "Taiko geth has {} blocks more than Taiko Inbox. Preparing batch for submission.",
                    self.preconfirmation_root.number - taiko_inbox_height
                );

                self.handle_unprocessed_blocks(
                    taiko_inbox_height,
                    self.preconfirmation_root.number,
                )
                .await?;
            }
            Ordering::Less => {
                // preconfirmation_root.number < taiko_inbox_height
                // extra block proposal was made by previous preconfer
                // return an error that will trigger a reorg.
                return Err(anyhow::anyhow!(
                    "❌ Unexpected block proposal was made by previous preconfer: preconfirming on {} but taiko_inbox_height is {}",
                    self.preconfirmation_root.number,
                    taiko_inbox_height
                ));
            }
            Ordering::Equal => {
                // preconfirmation_root.number == taiko_inbox_height
                // all good
            }
        }
        info!(
            "🔍 Verified block successfully: preconfirmation_root {}, hash: {} ",
            self.preconfirmation_root.number, self.preconfirmation_root.hash
        );

        metrics.inc_by_batch_recovered(self.batch_manager.get_number_of_batches());
        Ok(self.finalize_and_take_batches_to_send())
    }

    fn finalize_and_take_batches_to_send(&mut self) -> VecDeque<Batch> {
        self.batch_manager.finalize_current_batch();
        self.batch_manager.take_batches_to_send()
    }

    async fn handle_unprocessed_blocks(
        &mut self,
        taiko_inbox_height: u64,
        taiko_geth_height: u64,
    ) -> Result<(), Error> {
        let anchor_offset = self
            .batch_manager
            .get_l1_anchor_block_offset_for_l2_block(taiko_inbox_height + 1)
            .await?;
        // The first block anchor id is valid, so we can continue.
        if self
            .batch_manager
            .is_anchor_block_offset_valid(anchor_offset)
        {
            let start = std::time::Instant::now();
            // recover all missed l2 blocks
            for current_height in taiko_inbox_height + 1..=taiko_geth_height {
                self.batch_manager
                    .recover_from_l2_block(current_height)
                    .await?;
            }
            let elapsed = start.elapsed().as_millis();
            info!("Recovered in {} milliseconds", elapsed);
        } else {
            // Error will lead to a reorg
            return Err(anyhow::anyhow!(
                "Anchor offset exceeded during recovery: block {}, anchor_offset {}",
                taiko_inbox_height + 1,
                anchor_offset
            ));
        }

        Ok(())
    }
}
