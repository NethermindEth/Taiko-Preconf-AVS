pub mod batch_builder;

use crate::{ethereum_l1::EthereumL1, shared::l2_block::L2Block, taiko::Taiko};
use anyhow::Error;
use batch_builder::BatchBuilder;
use std::sync::Arc;
use tracing::{debug, warn};

/// Configuration for batching L2 transactions
#[derive(Clone)]
pub struct BatchBuilderConfig {
    /// Maximum size of the batch in bytes before sending
    pub max_bytes_size_of_batch: u64,
    /// Maximum number of blocks in a batch
    pub max_blocks_per_batch: u16,
    /// L1 slot duration in seconds
    pub l1_slot_duration_sec: u64,
    /// Maximum time shift between blocks in seconds
    pub max_time_shift_between_blocks_sec: u64,
    /// The max differences of the anchor height and the current block number
    pub max_anchor_height_offset: u64,
}

impl BatchBuilderConfig {
    pub fn is_within_block_limit(&self, num_blocks: u16) -> bool {
        num_blocks <= self.max_blocks_per_batch
    }

    pub fn is_within_bytes_limit(&self, total_bytes: u64) -> bool {
        total_bytes <= self.max_bytes_size_of_batch
    }
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
            // Handle the pending tx list from taiko geth
            debug!(
                "Received pending tx list length: {}, bytes length: {}",
                pending_tx_list.tx_list.len(),
                pending_tx_list.bytes_length
            );
            let l2_block = L2Block::new_from(pending_tx_list, preconfirmation_timestamp);
            self.add_new_l2_block(l2_block).await?;
        } else if self.is_empty_block_required(preconfirmation_timestamp) {
            // Handle time shift between blocks exceeded
            debug!("No pending txs, proposing empty block");
            let empty_block = L2Block::new_empty(preconfirmation_timestamp);
            self.add_new_l2_block(empty_block).await?;
        } else {
            debug!("No pending txs, skipping preconfirmation");
        }

        if self.batch_builder.is_grater_than_max_anchor_height_offset(
            self.ethereum_l1.execution_layer.get_l1_height().await?,
        ) {
            // Handle max anchor height offset exceeded
            debug!("Max anchor height offset exceeded");
            self.batch_builder.finalize_current_batch();

            if !submit {
                warn!("Max anchor height offset exceeded but submission is disabled");
            }
        }

        // Try to submit every time since we can have batches to send from preconfer only role.
        if submit {
            self.submit_batches(true).await?;
        }

        Ok(())
    }

    async fn add_new_l2_block(&mut self, l2_block: L2Block) -> Result<(), Error> {
        let anchor_block_id: u64 = self.consume_l2_block(l2_block.clone()).await?;

        self.taiko
            .advance_head_to_new_l2_block(l2_block, anchor_block_id)
            .await?;

        Ok(())
    }

    pub async fn consume_l2_block(&mut self, l2_block: L2Block) -> Result<u64, Error> {
        // If the L2 block can be added to the current batch, do so
        let anchor_block_id = if self.batch_builder.can_consume_l2_block(&l2_block) {
            self.batch_builder
                .add_l2_block_and_get_current_anchor_block_id(l2_block)?
        } else {
            // Otherwise, calculate the anchor block ID and create a new batch
            let anchor_block_id = self.calculate_anchor_block_id().await?;
            // Add the L2 block to the new batch
            self.batch_builder
                .create_new_batch_and_add_l2_block(anchor_block_id, l2_block);
            anchor_block_id
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
        self.batch_builder
            .submit_batches(self.ethereum_l1.clone(), submit_only_full_batches)
            .await
    }

    pub fn is_empty_block_required(&self, preconfirmation_timestamp: u64) -> bool {
        self.batch_builder
            .is_time_shift_between_blocks_expiring(preconfirmation_timestamp)
    }

    pub fn has_batches(&self) -> bool {
        !self.batch_builder.is_empty()
    }

    pub fn reset_builder(&mut self) {
        warn!("Resetting batch builder");
        self.batch_builder =
            batch_builder::BatchBuilder::new(self.batch_builder.get_config().clone());
    }
}
