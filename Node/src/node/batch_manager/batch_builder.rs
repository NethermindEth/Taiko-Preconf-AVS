use std::{collections::VecDeque, sync::Arc};

use crate::{ethereum_l1::EthereumL1, shared::l2_block::L2Block};
use anyhow::Error;
use tracing::{debug, trace, warn};

use super::BatchBuilderConfig;

#[derive(Default)]
pub struct Batch {
    pub l2_blocks: Vec<L2Block>,
    pub anchor_block_id: u64,
    pub total_bytes: u64,
}

pub struct BatchBuilder {
    config: BatchBuilderConfig,
    batches_to_send: VecDeque<Batch>,
    current_batch: Option<Batch>,
}

impl Drop for BatchBuilder {
    fn drop(&mut self) {
        debug!("BatchBuilder dropped!");
    }
}

impl BatchBuilder {
    pub fn new(config: BatchBuilderConfig) -> Self {
        Self {
            config,
            batches_to_send: VecDeque::new(),
            current_batch: None,
        }
    }

    /// Returns a reference to the batch builder configuration.
    ///
    /// This configuration is used to manage batching parameters.
    pub fn get_config(&self) -> &BatchBuilderConfig {
        &self.config
    }

    pub fn can_consume_l2_block(&self, l2_block: &L2Block) -> bool {
        self.current_batch
            .as_ref()
            .map_or(false, |batch| {
                // Check if the total bytes of the current batch after adding the new L2 block
                // is less than or equal to the max bytes size of the batch
                self.config.is_within_bytes_limit(batch.total_bytes + l2_block.prebuilt_tx_list.bytes_length)
                    // Check if the number of L2 blocks in the current batch after adding the new L2 block
                    // is less than or equal to the max blocks per batch
                    && self.config.is_within_block_limit(batch.l2_blocks.len() as u64 + 1)
            })
    }

    pub fn finalize_current_batch(&mut self) {
        if let Some(batch) = self.current_batch.take() {
            self.batches_to_send.push_back(batch);
        }
    }

    pub fn create_new_batch_and_add_l2_block(&mut self, anchor_block_id: u64, l2_block: L2Block) {
        self.finalize_current_batch();
        self.current_batch = Some(Batch {
            total_bytes: l2_block.prebuilt_tx_list.bytes_length,
            l2_blocks: vec![l2_block],
            anchor_block_id,
        });
    }

    /// Returns true if the block was added to the batch, false otherwise.
    pub fn add_l2_block_and_get_current_anchor_block_id(
        &mut self,
        l2_block: L2Block,
    ) -> Result<u64, Error> {
        if let Some(current_batch) = self.current_batch.as_mut() {
            current_batch.total_bytes += l2_block.prebuilt_tx_list.bytes_length;
            current_batch.l2_blocks.push(l2_block);
            debug!(
                "Added L2 block to batch: {} total bytes {}",
                current_batch.l2_blocks.len(),
                current_batch.total_bytes
            );
            Ok(current_batch.anchor_block_id)
        } else {
            Err(anyhow::anyhow!("No current batch"))
        }
    }

    pub fn is_empty(&self) -> bool {
        trace!(
            "batch_builder::is_empty: current_batch is none: {}, batches_to_send len: {}",
            self.current_batch.is_none(),
            self.batches_to_send.len()
        );
        self.current_batch.is_none() && self.batches_to_send.is_empty()
    }

    pub async fn submit_batches(
        &mut self,
        ethereum_l1: Arc<EthereumL1>,
        submit_only_full_batches: bool,
    ) -> Result<(), Error> {
        debug!(
            "Submitting batches: current_batch is none: {}, batches_to_send len: {}",
            self.current_batch.is_none(),
            self.batches_to_send.len()
        );
        if self.current_batch.is_some()
            && (!submit_only_full_batches
                || !self.config.is_within_block_limit(
                    self.current_batch.as_ref().unwrap().l2_blocks.len() as u64 + 1,
                ))
        {
            self.finalize_current_batch();
        }

        while let Some(batch) = self.batches_to_send.front() {
            ethereum_l1
                .execution_layer
                .send_batch_to_l1(batch.l2_blocks.clone(), batch.anchor_block_id)
                .await?;

            self.batches_to_send.pop_front();
        }

        Ok(())
    }

    pub fn is_time_shift_between_blocks_expiring(&self, current_l2_slot_timestamp: u64) -> bool {
        if let Some(current_batch) = self.current_batch.as_ref() {
            // l1_batches is not empty
            if let Some(last_block) = current_batch.l2_blocks.last() {
                if current_l2_slot_timestamp < last_block.timestamp_sec {
                    warn!("Preconfirmation timestamp is before the last block timestamp");
                    return false;
                }
                // is the last L1 slot to add an empty L2 block so we don't have a time shift overflow
                return self.is_the_last_l1_slot_to_add_an_empty_l2_block(
                    current_l2_slot_timestamp,
                    last_block.timestamp_sec,
                );
            }
        }
        false
    }

    fn is_the_last_l1_slot_to_add_an_empty_l2_block(
        &self,
        current_l2_slot_timestamp: u64,
        last_block_timestamp: u64,
    ) -> bool {
        current_l2_slot_timestamp - last_block_timestamp
            >= self.config.max_time_shift_between_blocks_sec - self.config.l1_slot_duration_sec
    }

    pub fn is_grater_than_max_anchor_height_offset(&self, current_l1_block: u64) -> bool {
        if let Some(current_batch) = self.current_batch.as_ref() {
            return current_batch.anchor_block_id + self.config.max_anchor_height_offset
                < current_l1_block;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_the_last_l1_slot_to_add_an_empty_l2_block() {
        let batch_builder = BatchBuilder::new(BatchBuilderConfig {
            max_bytes_size_of_batch: 1000,
            max_blocks_per_batch: 10,
            l1_slot_duration_sec: 12,
            max_time_shift_between_blocks_sec: 255,
            max_anchor_height_offset: 10,
        });

        assert_eq!(
            batch_builder.is_the_last_l1_slot_to_add_an_empty_l2_block(100, 0),
            false
        );
        assert_eq!(
            batch_builder.is_the_last_l1_slot_to_add_an_empty_l2_block(242, 0),
            false
        );
        assert!(batch_builder.is_the_last_l1_slot_to_add_an_empty_l2_block(243, 0));
        assert!(batch_builder.is_the_last_l1_slot_to_add_an_empty_l2_block(255, 0));
    }
}
