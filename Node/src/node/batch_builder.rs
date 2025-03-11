use anyhow::Error;
use std::sync::Arc;

use crate::{ethereum_l1::EthereumL1, taiko::l2_tx_lists::PreBuiltTxList};

/// Configuration for batching L2 transactions
struct BatchProposerConfig {
    /// Maximum size of the batch in bytes before sending
    max_size_of_batch: u64,
    /// Maximum number of blocks in a batch
    max_blocks_per_batch: usize,
}

impl BatchProposerConfig {
    fn new() -> Self {
        Self {
            max_size_of_batch: 100,    // TODO: Load from env
            max_blocks_per_batch: 100, // TODO: Load from L1 config
        }
    }
}

#[derive(Default)]
pub struct Batch {
    pub l2_blocks: Vec<PreBuiltTxList>,
    pub anchor_origin_height: u64,
    pub timestamp_sec: u64,
}

// TODO rename
// PendingTxLists is a Vec<PendingTxList>
// PendingTxList is a L2 block that we preconfirm
/// Proposes batched L2 transactions to L1
pub struct BatchBuilder {
    ethereum_l1: Arc<EthereumL1>,
    total_l2_blocks_size: u64,
    config: BatchProposerConfig,
    l1_batch: Batch,
}

pub enum BuilderState {
    InProgress,
    BatchCapacityFull(Batch),
    MaxBlocksPerBatch(Batch),
}

impl BatchBuilder {
    pub fn new(ethereum_l1: Arc<EthereumL1>) -> Self {
        let l1_batch = Batch {
            l2_blocks: Vec::new(),
            anchor_origin_height: 0,
            timestamp_sec: 0,
        };
        Self {
            ethereum_l1,
            total_l2_blocks_size: 0,
            config: BatchProposerConfig::new(),
            l1_batch,
        }
    }

    /// Handles incoming L2 blocks and batches them before sending to L1.
    /// If `submit` is true, immediately sends all l1_batches.
    pub async fn handle_l2_block(
        &mut self,
        l2_block: PreBuiltTxList,
        anchor_origin_height: u64,
        timestamp_sec: u64,
    ) -> Result<BuilderState, Error> {
        let mut state = BuilderState::InProgress;
        self.l1_batch.anchor_origin_height = anchor_origin_height;
        self.l1_batch.timestamp_sec = timestamp_sec;

        // Check if the current batch size is full before adding the new block
        if self.total_l2_blocks_size + l2_block.bytes_length > self.config.max_size_of_batch {
            state = BuilderState::BatchCapacityFull(self.build_batch());
        }

        self.total_l2_blocks_size += l2_block.bytes_length;
        self.l1_batch.l2_blocks.push(l2_block);

        // Check if we exceed the maximum number of blocks per batch
        if self.l1_batch.l2_blocks.len() >= self.config.max_blocks_per_batch {
            state = BuilderState::MaxBlocksPerBatch(self.build_batch());
        }

        // if submit {
        //     self.send_batches().await?;
        // }

        Ok(state)
    }

    /// Creates a batch from `l2_blocks` and prepares it for sending.
    fn build_batch(&mut self) -> Batch {
        tracing::debug!(
            "Building batch: {} blocks, total size: {} bytes",
            self.l1_batch.l2_blocks.len(),
            self.total_l2_blocks_size
        );

        self.total_l2_blocks_size = 0;
        std::mem::take(&mut self.l1_batch)
    }

    pub fn get_batch(&mut self) -> Option<Batch> {
        if self.l1_batch.l2_blocks.is_empty() {
            None
        } else {
            Some(self.build_batch())
        }
    }
}
