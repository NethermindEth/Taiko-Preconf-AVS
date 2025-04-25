use alloy::{consensus::BlockHeader, primitives::Address};
use anyhow::Error;
use std::sync::Arc;
use tracing::{debug, info};

use crate::taiko::Taiko;

use super::batch_manager::BatchManager;

pub struct Verifier {
    taiko: Arc<Taiko>,
    taiko_geth_height: u64,
    verified_height: u64,
    batch_manager: BatchManager,
    coinbase: Address,
}

impl Verifier {
    pub async fn new(taiko: Arc<Taiko>, batch_manager: BatchManager) -> Result<Self, Error> {
        let taiko_geth_height = taiko.get_latest_l2_block_id().await?;
        Ok(Self::new_with_taiko_height(
            taiko_geth_height,
            taiko,
            batch_manager,
        ))
    }

    pub fn new_with_taiko_height(
        taiko_geth_height: u64,
        taiko: Arc<Taiko>,
        batch_manager: BatchManager,
    ) -> Self {
        debug!(
            "Verifier created with taiko_geth_height: {}",
            taiko_geth_height
        );
        Self {
            taiko,
            taiko_geth_height,
            verified_height: 0,
            batch_manager,
            coinbase: Address::ZERO,
        }
    }

    pub async fn verify_submitted_blocks(&mut self, taiko_inbox_height: u64) -> Result<(), Error> {
        if self.taiko_geth_height > taiko_inbox_height
            && self.taiko_geth_height > self.verified_height
        {
            info!(
                "Taiko geth has {} blocks more than Taiko Inbox. Preparing batch for submission.",
                self.taiko_geth_height - taiko_inbox_height
            );

            let first_block = self
                .taiko
                .get_l2_block_by_number(taiko_inbox_height + 1, false)
                .await?;
            self.coinbase = first_block.header.beneficiary();

            self.handle_unprocessed_blocks(taiko_inbox_height, self.taiko_geth_height)
                .await?;
            self.verified_height = self.taiko_geth_height;
        }

        Ok(())
    }

    pub fn has_batches_to_submit(&self) -> bool {
        self.batch_manager.has_batches()
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
            info!("Recovering from L2 blocks for coinbase: {}", self.coinbase);
            for current_height in taiko_inbox_height + 1..=taiko_geth_height {
                self.batch_manager
                    .recover_from_l2_block(current_height)
                    .await?;
            }
            let elapsed = start.elapsed().as_secs();
            info!("Recovered in {} seconds", elapsed);
        }

        Ok(())
    }

    pub async fn try_submit_oldest_batch(&mut self) -> Result<(), Error> {
        self.batch_manager
            .try_submit_oldest_batch_with_coinbase(self.coinbase)
            .await
    }
}
