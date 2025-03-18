use crate::shared::l2_block::L2Block;
use tracing::{debug, warn};

/// Configuration for batching L2 transactions
struct BatchBuilderConfig {
    /// Maximum size of the batch in bytes before sending
    max_size_of_batch: u64,
    /// Maximum number of blocks in a batch
    max_blocks_per_batch: u64,
}

impl BatchBuilderConfig {
    fn new() -> Self {
        Self {
            max_size_of_batch: 1000000, // TODO: Load from env
            max_blocks_per_batch: 4,    // TODO: Load from L1 config
        }
    }
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
    pub fn new() -> Self {
        Self {
            config: BatchBuilderConfig::new(),
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
                <= self.config.max_size_of_batch
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
}
