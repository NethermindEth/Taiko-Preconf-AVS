pub mod batch_builder;

use crate::{ethereum_l1::EthereumL1, shared::l2_block::L2Block, taiko::Taiko};
use anyhow::Error;
use batch_builder::BatchBuilder;
use std::sync::Arc;
use tracing::debug;

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

    pub async fn preconfirm_block(&mut self, submit: bool) -> Result<(), Error> {
        let preconfirmation_timestamp =
            self.ethereum_l1.slot_clock.get_l2_slot_begin_timestamp()?;

        if let Some(pending_tx_list) = self.taiko.get_pending_l2_tx_list_from_taiko_geth().await? {
            debug!(
                "Received pending tx list length: {}, bytes length: {}",
                pending_tx_list.tx_list.len(),
                pending_tx_list.bytes_length
            );
            let l2_block = L2Block::new_from(pending_tx_list, preconfirmation_timestamp);
            self.process_new_l2_block(l2_block, submit).await?;
        } else if self.is_empty_block_required(preconfirmation_timestamp) {
            debug!("No pending txs, proposing empty block");
            let empty_block = L2Block::new_empty(preconfirmation_timestamp);
            self.process_new_l2_block(empty_block, submit).await?;
        } else {
            debug!("No pending txs, skipping preconfirmation");
        }

        Ok(())
    }

    async fn process_new_l2_block(&mut self, l2_block: L2Block, submit: bool) -> Result<(), Error> {
        let anchor_block_id: u64 = self.consume_l2_block(l2_block.clone()).await?;

        self.taiko
            .advance_head_to_new_l2_block(l2_block, anchor_block_id)
            .await?;

        if submit {
            self.submit_batches(true).await?;
        }

        Ok(())
    }

    pub async fn consume_l2_block(&mut self, l2_block: L2Block) -> Result<u64, Error> {
        let anchor_block_id = if !self.batch_builder.can_consume_l2_block(&l2_block) {
            let anchor_block_id = self.calculate_anchor_block_id().await?;
            self.batch_builder
                .create_new_batch_and_add_l2_block(anchor_block_id, l2_block);
            anchor_block_id
        } else {
            self.batch_builder
                .add_l2_block_and_get_current_anchor_block_id(l2_block)?
        };
        Ok(anchor_block_id)
    }

    async fn calculate_anchor_block_id(&self) -> Result<u64, Error> {
        let height_from_last_batch = self.taiko.get_last_synced_anchor_block_id().await?;
        let l1_height = self.ethereum_l1.execution_layer.get_l1_height().await?;
        let l1_height_with_lag = l1_height - self.l1_height_lag;

        Ok(std::cmp::max(height_from_last_batch, l1_height_with_lag))
    }

    pub async fn submit_batches(&mut self, submit_only_full_batches: bool) -> Result<(), Error> {
        debug!("Submitting batches");
        let batches: &mut Vec<batch_builder::Batch> = self.batch_builder.get_batches_mut();
        let batches_len = batches.len();

        for (i, batch) in batches.iter_mut().enumerate() {
            if batch.submitted || batch.is_empty() {
                continue;
            }

            let is_last_batch = i + 1 == batches_len;
            let skip_batch = is_last_batch
                && submit_only_full_batches
                && !batch.has_reached_max_number_of_blocks();

            if skip_batch {
                return Ok(());
            }

            self.ethereum_l1
                .execution_layer
                .send_batch_to_l1(batch.l2_blocks.clone(), batch.anchor_block_id)
                .await?;

            batch.submitted = true;
        }

        // Clear the batch builder since we have submitted all batches.
        debug!("Clearing batch builder");
        self.batch_builder =
            batch_builder::BatchBuilder::new(self.batch_builder.get_config().clone());

        Ok(())
    }

    pub fn is_empty_block_required(&self, preconfirmation_timestamp: u64) -> bool {
        self.batch_builder
            .is_time_shift_between_blocks_expiring(preconfirmation_timestamp)
    }

    pub fn has_batches(&self) -> bool {
        !self.batch_builder.is_current_l1_batch_empty()
    }
}
