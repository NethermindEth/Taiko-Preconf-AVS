use alloy::{consensus::BlockHeader, primitives::Address};
use anyhow::Error;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::taiko::Taiko;

use super::batch_manager::BatchManager;

pub struct Verifier {
    taiko: Arc<Taiko>,
    taiko_geth_height: u64,
    verified_height: u64,
}

impl Verifier {
    pub async fn new(taiko: Arc<Taiko>) -> Result<Self, Error> {
        let taiko_geth_height = taiko.get_latest_l2_block_id().await?;
        Ok(Self::with_taiko_height(taiko_geth_height, taiko))
    }

    pub fn with_taiko_height(taiko_geth_height: u64, taiko: Arc<Taiko>) -> Self {
        debug!(
            "Verifier created with taiko_geth_height: {}",
            taiko_geth_height
        );
        Self {
            taiko,
            taiko_geth_height,
            verified_height: 0,
        }
    }

    pub async fn verify_submitted_blocks(
        &mut self,
        mut batch_manager: BatchManager,
        taiko_inbox_height: u64,
    ) -> Result<(), Error> {
        if self.taiko_geth_height > taiko_inbox_height
            && self.taiko_geth_height > self.verified_height
        {
            info!(
                "Taiko geth has {} blocks more than Taiko Inbox. Trying to submit these blocks.",
                self.taiko_geth_height - taiko_inbox_height
            );

            let first_block = self
                .taiko
                .get_l2_block_by_number(taiko_inbox_height + 1, false)
                .await?;
            let coinbase = first_block.header.beneficiary();

            handle_unprocessed_blocks(
                &mut batch_manager,
                taiko_inbox_height,
                self.taiko_geth_height,
                coinbase,
            )
            .await?;
            self.verified_height = self.taiko_geth_height;
        }

        Ok(())
    }
}

pub async fn handle_unprocessed_blocks(
    batch_manager: &mut BatchManager,
    taiko_inbox_height: u64,
    taiko_geth_height: u64,
    coinbase: Address,
) -> Result<(), Error> {
    let anchor_offset = batch_manager
        .get_anchor_block_offset(taiko_inbox_height + 1)
        .await?;
    let mut extra_slots: u64 = 0;
    // The first block anchor id is valid, so we can continue.
    if batch_manager.is_anchor_block_offset_valid(anchor_offset) {
        let start = std::time::Instant::now();
        // recover all missed l2 blocks
        info!("Recovering from L2 blocks for coinbase: {}", coinbase);
        for current_height in taiko_inbox_height + 1..=taiko_geth_height {
            batch_manager.recover_from_l2_block(current_height).await?;
        }
        let elapsed = start.elapsed().as_secs();
        extra_slots = elapsed / batch_manager.get_config().l1_slot_duration_sec;
        info!(
            "Recovered in {} seconds (extra_slots = {})",
            elapsed, extra_slots
        );
    }
    if batch_manager.is_anchor_block_offset_valid(anchor_offset + extra_slots) {
        // TODO calculate batch params and decide is it possible to continue with it, be careful with timeShift
        // Sould be fixed with https://github.com/NethermindEth/Taiko-Preconf-AVS/issues/303
        // Now just submit all the batches
        info!("Submit batches");
        batch_manager
            .try_submit_batches_with_coinbase(coinbase)
            .await?;
    } else {
        // The first block anchor id is not valid
        // Just do force reorg
        warn!("Triggering L2 reorg");
        return Err(anyhow::anyhow!(
            "Error: L2 chain state may be inconsistent."
        ));
    }

    Ok(())
}
