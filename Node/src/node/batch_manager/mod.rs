pub mod batch_builder;

use crate::{ethereum_l1::EthereumL1, shared::l2_block::L2Block, taiko::Taiko};
use anyhow::Error;
use batch_builder::BatchBuilder;
use std::sync::Arc;
use tracing::{debug, info};

/// Configuration for batching L2 transactions
#[derive(Clone)]
pub struct BatchBuilderConfig {
    /// Maximum size of the batch in bytes before sending
    pub max_bytes_size_of_batch: u64,
    /// Maximum number of blocks in a batch
    pub max_blocks_per_batch: u64,
    /// L1 slot duration in seconds
    pub l1_slot_duration_sec: u64,
    /// Maximum time shift between blocks in seconds
    pub max_time_shift_between_blocks_sec: u64,
}

pub struct BatchManager {
    batch_builder: BatchBuilder,
    ethereum_l1: Arc<EthereumL1>,
    taiko: Arc<Taiko>,
    l1_height_lag: u64,
}

impl BatchManager {
    pub fn new(
        l1_height_lag: u64,
        config: BatchBuilderConfig,
        ethereum_l1: Arc<EthereumL1>,
        taiko: Arc<Taiko>,
    ) -> Self {
        Self {
            batch_builder: BatchBuilder::new(config),
            ethereum_l1,
            taiko,
            l1_height_lag,
        }
    }

    pub async fn consume_l2_block(&mut self, l2_block: L2Block) -> Result<u64, Error> {
        let anchor_block_id = if !self.batch_builder.can_consume_l2_block(&l2_block) {
            let anchor_block_id = self.calculate_anchor_block_id().await?;
            self.batch_builder
                .create_new_batch_and_add_l2_block(anchor_block_id, l2_block);
            anchor_block_id
        } else {
            self.batch_builder
                .add_l2_block_and_get_current_anchor_block_id(l2_block)
        };
        Ok(anchor_block_id)
    }

    async fn calculate_anchor_block_id(&self) -> Result<u64, Error> {
        let height_from_last_batch = self.taiko.get_last_anchor_block_id().await?;
        let l1_height = self.ethereum_l1.execution_layer.get_l1_height().await?;
        let l1_height_with_lag = l1_height - self.l1_height_lag;

        Ok(std::cmp::max(height_from_last_batch, l1_height_with_lag))
    }

    pub async fn submit_batches(&mut self, submit_only_full_batches: bool) -> Result<(), Error> {
        debug!("Submitting batches");
        if let Some(batches) = self.batch_builder.get_batches() {
            let batches_len = batches.len();
            for (i, batch) in batches.iter_mut().enumerate() {
                if batch.submitted {
                    continue;
                }
                if batch.l2_blocks.is_empty() || (submit_only_full_batches && i == batches_len) {
                    return Ok(());
                }
                self.ethereum_l1
                    .execution_layer
                    .send_batch_to_l1(batch.l2_blocks.clone(), batch.anchor_block_id)
                    .await?;
                batch.submitted = true;
                debug!("Submitted batch.");
            }
            info!("All batches submitted");
            // since all batches are submitted including not full ones, we can clear the batch builder
            if !submit_only_full_batches {
                self.batch_builder =
                    batch_builder::BatchBuilder::new(self.batch_builder.get_config().clone());
            }
        }
        Ok(())
    }

    pub fn is_need_empty_block(&self, preconfirmation_timestamp: u64) -> bool {
        self.batch_builder
            .is_time_shift_between_blocks_expiring(preconfirmation_timestamp)
    }

    pub fn has_batches(&self) -> bool {
        !self.batch_builder.is_current_l1_batch_empty()
    }

    pub fn reset_builder(&mut self) {
        self.batch_builder =
            batch_builder::BatchBuilder::new(self.batch_builder.get_config().clone());
    }
}
