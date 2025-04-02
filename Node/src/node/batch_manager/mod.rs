pub mod batch_builder;

use crate::{
    ethereum_l1::EthereumL1,
    shared::{l2_block::L2Block, l2_tx_lists::PreBuiltTxList},
    taiko::Taiko,
};
use alloy::{
    consensus::Transaction,
    primitives::{aliases::U96, U256},
};
use anyhow::Error;
use batch_builder::BatchBuilder;
use futures_util::future::try_join_all;
use std::sync::Arc;
use tracing::{debug, info, warn};

// TODO move to config
const MIN_SLOTS_TO_PROPOSE: u64 = 3; // Minimum number of slots required to propose a batch on L1

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
    /// The amount of Taiko token as a prover liveness bond per batch.
    pub liveness_bond_base: U96,
    /// The amount of Taiko token as a prover liveness bond per block.
    pub liveness_bond_per_block: U96,
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
    pub taiko: Arc<Taiko>,
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

    pub async fn is_bond_balance_valid(&self, blocks_to_add: u64) -> Result<bool, Error> {
        let balance = self.ethereum_l1.execution_layer.get_bonds_balance().await?;
        debug!("Bond balance: {}", balance);

        let required_balance = self.batch_builder.get_config().liveness_bond_base
            + self.batch_builder.get_config().liveness_bond_per_block
                * U96::from(
                    self.batch_builder.get_current_batch_blocks_count() + blocks_to_add as usize,
                );
        debug!("Required bond balance: {}", required_balance);

        if balance < U256::from(required_balance) {
            return Ok(false);
        }
        Ok(true)
    }

    pub async fn recover_from_l2_block(&mut self, block_height: u64) -> Result<(), Error> {
        debug!("Recovering from L2 block {}", block_height);
        let block = self.taiko.get_l2_block_by_number(block_height).await?;
        let tx_hashes = block
            .transactions
            .as_hashes()
            .ok_or_else(|| anyhow::anyhow!("recover_from_l2_block: No transactions in block"))?;

        let (anchor_tx_hash, txs_hashes) = tx_hashes.split_first().ok_or_else(|| {
            anyhow::anyhow!("recover_from_l2_block: No anchor transaction in block")
        })?;

        let anchor_tx = self.taiko.get_transaction_by_hash(*anchor_tx_hash).await?;
        let anchor_block_id = Taiko::decode_anchor_tx_data(anchor_tx.input())?;
        debug!(
            "Recovering from L2 block {} with anchor block id {}",
            block_height, anchor_block_id
        );

        // Fetch transactions concurrently
        let tx_futures = txs_hashes
            .iter()
            .map(|tx_hash| self.taiko.get_transaction_by_hash(*tx_hash));
        let txs: Vec<alloy::rpc::types::Transaction> = try_join_all(tx_futures).await?;

        debug!(
            "Recovering from L2 block {} with {} transactions and timestamp {}",
            block_height,
            txs.len(),
            block.header.timestamp
        );

        self.batch_builder
            .recover_from(txs, anchor_block_id, block.header.timestamp);

        Ok(())
    }

    pub async fn is_block_valid(&self, block_height: u64) -> Result<bool, Error> {
        debug!("is_block_valid: Checking L2 block {}", block_height);
        let block = self.taiko.get_l2_block_by_number(block_height).await?;

        let anchor_tx_hash = block
            .transactions
            .as_hashes()
            .and_then(|txs| txs.first())
            .ok_or_else(|| anyhow::anyhow!("is_block_valid: No transactions in block"))?;

        let anchor_tx = self.taiko.get_transaction_by_hash(*anchor_tx_hash).await?;
        let anchor_block_id = Taiko::decode_anchor_tx_data(anchor_tx.input())?;

        debug!(
            "is_block_valid: L2 block {} has anchor block id {}",
            block_height, anchor_block_id
        );

        let l1_height = self.ethereum_l1.execution_layer.get_l1_height().await?;
        let anchor_offset = l1_height - anchor_block_id;
        let max_anchor_height_offset = self
            .ethereum_l1
            .execution_layer
            .get_pacaya_config()
            .maxAnchorHeightOffset;
        if anchor_offset + MIN_SLOTS_TO_PROPOSE > max_anchor_height_offset {
            warn!(
                "Skip recovery! Reorg detected! Anchor height offset is greater than max anchor height offset. L1 height: {}, anchor block id: {}, anchor height offset: {}, max anchor height offset: {}",
                l1_height, anchor_block_id, anchor_offset, max_anchor_height_offset
            );
            return Ok(false);
        }

        info!(
            "is_block_valid: L1 height: {}, anchor block id: {}, anchor height offset: {}, max anchor height offset: {}",
            l1_height, anchor_block_id, anchor_offset, max_anchor_height_offset
        );

        Ok(true)
    }

    pub async fn preconfirm_block(
        &mut self,
        submit: bool,
        pending_tx_list: Option<PreBuiltTxList>,
    ) -> Result<(), Error> {
        let preconfirmation_timestamp =
            self.ethereum_l1.slot_clock.get_l2_slot_begin_timestamp()?;

        if let Some(pending_tx_list) = pending_tx_list {
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
            info!("📈 Maximum allowed anchor height offset exceeded, finalizing current batch.");
            self.batch_builder.finalize_current_batch();

            if !submit {
                warn!("Max anchor height offset exceeded but submission is disabled");
            }
        }

        // Try to submit every time since we can have batches to send from preconfer only role.
        if submit {
            self.try_submit_batches(true).await?;
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

    pub async fn try_submit_batches(
        &mut self,
        submit_only_full_batches: bool,
    ) -> Result<(), Error> {
        self.batch_builder
            .try_submit_batches(self.ethereum_l1.clone(), submit_only_full_batches)
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
