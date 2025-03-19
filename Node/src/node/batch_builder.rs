use crate::shared::l2_block::L2Block;
use tracing::{debug, warn};

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

#[derive(Default)]
pub struct Batch {
    pub l2_blocks: Vec<L2Block>,
    pub anchor_block_id: u64,
    pub total_l2_blocks_size: u64,
    pub submitted: bool,
    pub max_blocks_per_batch: u64,
}

impl Batch {
    pub fn is_full(&self) -> bool {
        if self.l2_blocks.len() > self.max_blocks_per_batch as usize {
            warn!(
                "Batch size grater then max_blocks_per_batch: {} > {}",
                self.l2_blocks.len(),
                self.max_blocks_per_batch
            );
        }
        self.l2_blocks.len() == self.max_blocks_per_batch as usize
    }
}

pub struct BatchBuilder {
    config: BatchBuilderConfig,
    l1_batches: Vec<Batch>,
    pub current_l1_batch_index: usize,
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
            l1_batches: vec![],
            current_l1_batch_index: 0,
        }
    }

    fn get_current_batch(&self) -> &Batch {
        &self.l1_batches[self.current_l1_batch_index]
    }

    fn get_current_batch_mut(&mut self) -> &mut Batch {
        &mut self.l1_batches[self.current_l1_batch_index]
    }

    pub fn can_consume_l2_block(&self, l2_block: &L2Block) -> bool {
        self.l1_batches.len() > 0
            && self.get_current_batch().total_l2_blocks_size
                + l2_block.prebuilt_tx_list.bytes_length
                <= self.config.max_bytes_size_of_batch
            && !self.get_current_batch().is_full()
    }

    pub fn create_new_batch_and_add_l2_block(&mut self, anchor_block_id: u64, l2_block: L2Block) {
        let l1_batch = Batch {
            l2_blocks: vec![l2_block],
            anchor_block_id,
            submitted: false,
            max_blocks_per_batch: self.config.max_blocks_per_batch,
            total_l2_blocks_size: 1,
        };
        self.l1_batches.push(l1_batch);
        self.current_l1_batch_index = self.l1_batches.len() - 1;
    }

    /// Returns true if the block was added to the batch, false otherwise.
    pub fn add_l2_block_and_get_current_anchor_block_id(&mut self, l2_block: L2Block) -> u64 {
        let current_batch = self.get_current_batch_mut();
        current_batch.total_l2_blocks_size += l2_block.prebuilt_tx_list.bytes_length;
        current_batch.l2_blocks.push(l2_block);
        debug!("Added L2 block to batch: {}", current_batch.l2_blocks.len());
        current_batch.anchor_block_id
    }

    pub fn is_current_l1_batch_empty(&self) -> bool {
        self.l1_batches.is_empty() || self.get_current_batch().l2_blocks.is_empty()
    }

    pub fn get_batches(&mut self) -> Option<&mut Vec<Batch>> {
        if self.l1_batches.is_empty() || self.get_current_batch().l2_blocks.is_empty() {
            None
        } else {
            Some(&mut self.l1_batches)
        }
    }

    pub fn is_time_shift_between_blocks_expiring(
        &mut self,
        current_l2_slot_timestamp: u64,
    ) -> bool {
        if self.l1_batches.is_empty()
            || self.get_current_batch().l2_blocks.is_empty()
            || self.get_current_batch().submitted
        {
            return false;
        }

        if let Some(last_block) = self.get_current_batch().l2_blocks.last() {
            if current_l2_slot_timestamp < last_block.timestamp_sec {
                warn!("Preconfirmation timestamp is before the last block timestamp");
                return false;
            }
            // is the last L1 slot to add an empty L2 block so we don't have a time shift overflow
            self.is_the_last_l1_slot_to_add_an_empty_l2_block(
                current_l2_slot_timestamp,
                last_block.timestamp_sec,
            )
        } else {
            false
        }
    }

    fn is_the_last_l1_slot_to_add_an_empty_l2_block(
        &self,
        current_l2_slot_timestamp: u64,
        last_block_timestamp: u64,
    ) -> bool {
        current_l2_slot_timestamp - last_block_timestamp
            >= self.config.max_time_shift_between_blocks_sec - self.config.l1_slot_duration_sec
    }
}

mod tests {
    use super::*;

    #[test]
    fn test_is_the_last_l1_slot_to_add_an_empty_l2_block() {
        let batch_builder = BatchBuilder::new(BatchBuilderConfig {
            max_bytes_size_of_batch: 1000,
            max_blocks_per_batch: 10,
            l1_slot_duration_sec: 12,
            max_time_shift_between_blocks_sec: 255,
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
