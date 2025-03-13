use tracing::debug;
use crate::shared::l2_block::L2Block;

/// Configuration for batching L2 transactions
struct BatchBuilderConfig {
    /// Maximum size of the batch in bytes before sending
    max_size_of_batch: u64,
    /// Maximum number of blocks in a batch
    max_blocks_per_batch: usize,
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
    pub is_full: bool
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
        let l1_batch = Batch {
            l2_blocks: Vec::new(),
            anchor_block_id: 0,
            submitted: false,
            is_full: false,
            total_l2_blocks_size: 0,
        };
        Self {
            config: BatchBuilderConfig::new(),
            l1_batches: vec![l1_batch],
            current_l1_batch_index: 0,
        }
    }

    fn get_current_batch(&mut self) -> &mut Batch {
        &mut self.l1_batches[self.current_l1_batch_index]
    }

    fn can_consume_l2_block(&mut self, l2_block: &L2Block) -> bool {
        self.get_current_batch().total_l2_blocks_size + l2_block.prebuilt_tx_list.bytes_length
            <= self.config.max_size_of_batch
            && self.get_current_batch().l2_blocks.len() < self.config.max_blocks_per_batch
    }

    pub fn create_new_batch_if_cant_consume(&mut self, l2_block: &L2Block) {
        if self.can_consume_l2_block(l2_block) {
            self.get_current_batch().is_full = true;
            return;
        }
        let l1_batch = Batch {
            l2_blocks: Vec::new(),
            anchor_block_id: 0,
            submitted: false,
            is_full: false,
            total_l2_blocks_size: 0,
        };
        self.l1_batches.push(l1_batch);
        self.current_l1_batch_index += 1;
    }

    /// Returns true if the block was added to the batch, false otherwise.
    pub fn add_l2_block(&mut self, l2_block: L2Block) {
        self.get_current_batch().total_l2_blocks_size += l2_block.prebuilt_tx_list.bytes_length;
        self.get_current_batch().l2_blocks.push(l2_block);
        debug!("Added L2 block to batch: {}", self.get_current_batch().l2_blocks.len());
        if self.get_current_batch().l2_blocks.len() == self.config.max_blocks_per_batch {
            self.get_current_batch().is_full = true;
        }
    }

    pub fn is_new_batch(&mut self) -> bool {
        self.get_current_batch().l2_blocks.is_empty()
    }

    pub fn set_anchor_id(&mut self, anchor_block_id: u64) {
        self.get_current_batch().anchor_block_id = anchor_block_id;
    }

    pub fn get_anchor_block_id(&mut self) -> u64 {
        self.get_current_batch().anchor_block_id
    }

    pub fn is_current_l1_batch_empty(&mut self) -> bool {
        self.get_current_batch().l2_blocks.is_empty()
    }

    pub fn get_batches(&mut self) -> Option<&mut Vec<Batch>> {
        if self.l1_batches.len() == 1 && self.get_current_batch().l2_blocks.is_empty() {
            None
        } else {
            Some(&mut self.l1_batches)
        }
    }
}
