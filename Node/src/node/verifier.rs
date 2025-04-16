use alloy::{consensus::BlockHeader, primitives::Address};
use anyhow::Error;
use std::sync::Arc;
use tracing::{debug, info};

use crate::{ethereum_l1::execution_layer::ExecutionLayer, taiko::Taiko};

use super::batch_manager::BatchManager;

pub struct Verifier {
    execution_layer: Arc<ExecutionLayer>,
    taiko: Arc<Taiko>,
    taiko_geth_height: u64,
    verified_height: u64,
}

impl Verifier {
    pub async fn new(
        execution_layer: Arc<ExecutionLayer>,
        taiko: Arc<Taiko>,
    ) -> Result<Self, Error> {
        let taiko_geth_height = taiko.get_latest_l2_block_id().await?;
        debug!(
            "Verifier created with taiko_geth_height: {}",
            taiko_geth_height
        );
        Ok(Self {
            execution_layer,
            taiko,
            taiko_geth_height,
            verified_height: 0,
        })
    }

    pub async fn verify_submitted_blocks(
        &mut self,
        mut batch_manager: BatchManager,
    ) -> Result<(), Error> {
        let taiko_inbox_height = self
            .execution_layer
            .get_l2_height_from_taiko_inbox()
            .await?;

        if self.taiko_geth_height > taiko_inbox_height
            && self.taiko_geth_height > self.verified_height
        {
            info!(
                "Taiko geth has {} blocks more than Taiko Inbox. Trying to submit these blocks.",
                self.taiko_geth_height - taiko_inbox_height
            );

            let first_block = self
                .taiko
                .get_l2_block_by_number(taiko_inbox_height + 1)
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
    // The first block anchor id is valid, so we can continue.
    if batch_manager.is_block_valid(taiko_inbox_height + 1).await? {
        // recover all missed l2 blocks
        info!("Recovering from L2 blocks for coinbase: {}", coinbase);
        for current_height in taiko_inbox_height + 1..=taiko_geth_height {
            batch_manager.recover_from_l2_block(current_height).await?;
        }
        // TODO calculate batch params and decide is it possible to continue with it, be careful with timeShift
        // Sould be fixed with https://github.com/NethermindEth/Taiko-Preconf-AVS/issues/303
        // Now just submit all the batches
        batch_manager
            .try_submit_batches_with_coinbase(coinbase)
            .await?;
    } else {
        // The first block anchor id is not valid
        // TODO reorg + reanchor + preconfirm again
        // Just do force reorg
        info!("Triggering L2 reorg");
        batch_manager
            .taiko
            .trigger_l2_reorg(taiko_inbox_height)
            .await?;
    }

    Ok(())
}
