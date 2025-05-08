use alloy::primitives::B256;
use anyhow::Error;
use std::{cmp::Ordering, sync::Arc};
use tracing::{debug, info};

use crate::{taiko::Taiko, utils::types::Slot};

use super::batch_manager::BatchManager;

use crate::Metrics;

struct PreconfirmationRootBlock {
    number: u64,
    hash: B256,
}

pub struct Verifier {
    taiko: Arc<Taiko>,
    preconfirmation_root: PreconfirmationRootBlock,
    verified_height: u64,
    batch_manager: BatchManager,
    verification_slot: Slot,
}

impl Verifier {
    pub async fn new_with_taiko_height(
        taiko_geth_height: u64,
        taiko: Arc<Taiko>,
        batch_manager: BatchManager,
        verification_slot: Slot,
    ) -> Result<Self, Error> {
        let hash = taiko.get_l2_block_hash(taiko_geth_height).await?;
        debug!(
            "Verifier created with taiko_geth_height: {}, hash: {}, verification_slot: {}",
            taiko_geth_height, hash, verification_slot
        );
        Ok(Self {
            taiko,
            preconfirmation_root: PreconfirmationRootBlock {
                number: taiko_geth_height,
                hash,
            },
            verified_height: 0,
            batch_manager,
            verification_slot,
        })
    }

    pub fn is_slot_valid(&self, current_slot: Slot) -> bool {
        current_slot >= self.verification_slot
    }

    pub fn get_verification_slot(&self) -> Slot {
        self.verification_slot
    }

    pub async fn verify_submitted_blocks(
        &mut self,
        taiko_inbox_height: u64,
        metrics: Arc<Metrics>,
    ) -> Result<(), Error> {
        if self.preconfirmation_root.number > self.verified_height {
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
                    return Err(anyhow::anyhow!("❌ Unexpected block proposal was made by previous preconfer: preconfirming on {} but taiko_inbox_height is {}", self.preconfirmation_root.number, taiko_inbox_height));
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
            self.verified_height = taiko_inbox_height;

            metrics.inc_by_batch_recovered(self.get_number_of_batches());
        }

        Ok(())
    }

    pub fn has_batches_to_submit(&self) -> bool {
        self.batch_manager.has_batches()
    }

    pub fn get_number_of_batches(&self) -> u64 {
        self.batch_manager.get_number_of_batches()
    }

    pub async fn handle_unprocessed_blocks(
        &mut self,
        taiko_inbox_height: u64,
        taiko_geth_height: u64,
    ) -> Result<(), Error> {
        let anchor_offset = self
            .batch_manager
            .get_anchor_block_offset(taiko_inbox_height + 1)
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

    pub async fn try_submit_oldest_batch(&mut self) -> Result<(), Error> {
        self.batch_manager.try_submit_oldest_batch(false).await
    }
}
