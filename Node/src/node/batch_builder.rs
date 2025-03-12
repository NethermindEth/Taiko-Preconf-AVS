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
pub struct Batch<'a> {
    pub l2_blocks: Vec<L2Block>,
    pub anchor_block_id: u64,
    pub total_l2_blocks_size: u64,
    pub submitted: &'a mut bool
}

impl Batch {
    pub fn get_last_l2_block_timestamp(&self) -> u64 {
        self.l2_blocks.last().unwrap().timestamp_sec
    }
}

pub struct BatchBuilder<'a> {
    config: BatchBuilderConfig,
    l1_batches: Vec<&'a mut Batch<'a>>,
    current_l1_batch: Batch<'a>,
}

impl BatchBuilder {
    pub fn new() -> Self {
        let mut l1_batch = Batch {
            l2_blocks: Vec::new(),
            anchor_block_id: 0,
            submitted: &mut false,
            total_l2_blocks_size: 0,
        };
        Self {
            config: BatchBuilderConfig::new(),
            l1_batches: vec![&mut l1_batch],
            current_l1_batch: l1_batch,
        }
    }

    fn can_consume_l2_block(&self, l2_block: &L2Block) -> bool {
        self.current_l1_batch.total_l2_blocks_size + l2_block.prebuilt_tx_list.bytes_length
            <= self.config.max_size_of_batch
            && self.current_l1_batch.l2_blocks.len() < self.config.max_blocks_per_batch
    }

    pub fn create_new_batch_if_cant_consume(&mut self, l2_block: &L2Block) {
        if self.can_consume_l2_block(l2_block) {
            return;
        }
        let mut l1_batch = Batch {
            l2_blocks: Vec::new(),
            anchor_block_id: 0,
            submitted: &mut false,
            total_l2_blocks_size: 0,
        };
        self.l1_batches.push(&mut l1_batch);
        self.current_l1_batch = l1_batch;
    }

    /// Returns true if the block was added to the batch, false otherwise.
    pub fn add_l2_block(&mut self, l2_block: L2Block) {
        self.current_l1_batch.total_l2_blocks_size += l2_block.prebuilt_tx_list.bytes_length;
        self.current_l1_batch.l2_blocks.push(l2_block);
        tracing::debug!("Added L2 block to batch: {}", self.current_l1_batch.l2_blocks.len());
    }

    pub fn is_new_batch(&self) -> bool {
        self.current_l1_batch.l2_blocks.is_empty()
    }

    pub fn set_anchor_id(&mut self, anchor_block_id: u64) {
        self.current_l1_batch.anchor_block_id = anchor_block_id;
    }

    pub fn get_anchor_block_id(&self) -> u64 {
        self.current_l1_batch.anchor_block_id
    }

    /// Creates a batch from `l2_blocks` and prepares it for sending.
    fn build_batch(&mut self) -> Batch {
        tracing::debug!(
            "Building batch: {} blocks, total size: {} bytes",
            self.current_l1_batch.l2_blocks.len(),
            self.current_l1_batch.total_l2_blocks_size
        );

        self.current_l1_batch.total_l2_blocks_size = 0;
        std::mem::take(&mut self.current_l1_batch)
    }

    pub fn get_batches(&self) -> Option<Vec<&mut Batch>> {
        if self.l1_batches.len() == 1 && self.current_l1_batch.l2_blocks.is_empty() {
            None
        } else {
            Some(self.l1_batches.clone())
        }
    }
}
