use anyhow::Error;
use std::sync::Arc;

use crate::{ethereum_l1::EthereumL1, taiko::l2_tx_lists::PendingTxLists};

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

// TODO rename
// PendingTxLists is a Vec<PendingTxList>
// PendingTxList is a L2 block that we preconfirm
/// Proposes batched L2 transactions to L1
pub struct BatchProposer {
    ethereum_l1: Arc<EthereumL1>,
    l2_blocks: PendingTxLists,
    total_l2_blocks_size: u64,
    config: BatchProposerConfig,
    l1_batches: Vec<PendingTxLists>,
}

impl BatchProposer {
    pub fn new(ethereum_l1: Arc<EthereumL1>) -> Self {
        Self {
            ethereum_l1,
            l2_blocks: Vec::new(),
            total_l2_blocks_size: 0,
            config: BatchProposerConfig::new(),
            l1_batches: Vec::new(),
        }
    }

    /// Handles incoming L2 blocks and batches them before sending to L1.
    /// If `submit` is true, immediately sends all l1_batches.
    pub async fn handle_l2_blocks(
        &mut self,
        l2_blocks: PendingTxLists,
        submit: bool,
    ) -> Result<(), Error> {
        for l2_block in l2_blocks {
            // Check if the current batch size is full before adding the new block
            if self.total_l2_blocks_size + l2_block.bytes_length > self.config.max_size_of_batch {
                self.build_batch();
            }

            self.total_l2_blocks_size += l2_block.bytes_length;
            self.l2_blocks.push(l2_block);

            // Check if we exceed the maximum number of blocks per batch
            if self.l2_blocks.len() >= self.config.max_blocks_per_batch {
                self.build_batch();
            }
        }

        if submit {
            self.send_batches().await?;
        }

        Ok(())
    }

    /// Sends all accumulated L2 batches to L1.
    async fn send_batches(&mut self) -> Result<(), Error> {
        if self.l1_batches.is_empty() {
            return Ok(()); // No l1_batches to send
        }

        // Fetch nonce from L1
        // TODO handle nonce correctly for few batches in one L1 slot
        let mut nonce = self
            .ethereum_l1
            .execution_layer
            .get_preconfer_nonce()
            .await?;

        // Send each batch to L1
        for tx in std::mem::take(&mut self.l1_batches) {
            self.ethereum_l1
                .execution_layer
                .send_batch_to_l1(tx, nonce)
                .await?;
            nonce += 1;
        }

        Ok(())
    }

    /// Creates a batch from `l2_blocks` and prepares it for sending.
    fn build_batch(&mut self) {
        tracing::debug!(
            "Building batch: {} blocks, total size: {} bytes",
            self.l2_blocks.len(),
            self.total_l2_blocks_size
        );

        if !self.l2_blocks.is_empty() {
            self.l1_batches.push(std::mem::take(&mut self.l2_blocks));
            self.total_l2_blocks_size = 0;
        }
    }

    /// Forces all L2 blocks to be batched and sent to L1.
    pub async fn submit_all(&mut self) -> Result<(), Error> {
        if !self.l2_blocks.is_empty() {
            self.build_batch();
        }

        self.send_batches().await
    }
}
