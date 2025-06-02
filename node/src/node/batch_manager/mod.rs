pub mod batch_builder;

use crate::{
    ethereum_l1::EthereumL1,
    shared::{l2_block::L2Block, l2_slot_info::L2SlotInfo, l2_tx_lists::PreBuiltTxList},
    taiko::preconf_blocks::BuildPreconfBlockResponse,
    taiko::{Taiko, config::OperationType},
};
use alloy::{consensus::BlockHeader, consensus::Transaction, primitives::Address};
use anyhow::Error;
use batch_builder::{Batch, BatchBuilder};
use std::{collections::VecDeque, sync::Arc};
use tracing::{debug, error, info, trace, warn};

// TODO move to config
const MIN_SLOTS_TO_PROPOSE: u64 = 5; // Minimum number of slots required to propose a batch on L1

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
    /// Default coinbase
    pub default_coinbase: Address,
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
            batch_builder: BatchBuilder::new(config, ethereum_l1.slot_clock.clone()),
            ethereum_l1,
            taiko,
            l1_height_lag,
        }
    }

    pub async fn recover_from_l2_block(&mut self, block_height: u64) -> Result<(), Error> {
        debug!("Recovering from L2 block {}", block_height);
        let block = self
            .taiko
            .get_l2_block_by_number(block_height, true)
            .await?;
        let (anchor_tx, txs) = match block.transactions.as_transactions() {
            Some(txs) => txs
                .split_first()
                .ok_or_else(|| anyhow::anyhow!("Cannot get anchor transaction from block"))?,
            None => return Err(anyhow::anyhow!("No transactions in block")),
        };

        let coinbase = block.header.beneficiary();

        let anchor_block_id = Taiko::decode_anchor_tx_data(anchor_tx.input())?;
        debug!(
            "Recovering from L2 block {}, anchor block id {}, timestamp {}, coinbase {}, transactions {}",
            block_height,
            anchor_block_id,
            block.header.timestamp,
            coinbase,
            txs.len()
        );

        let anchor_block_timestamp_sec = self
            .ethereum_l1
            .execution_layer
            .get_block_timestamp_by_id(anchor_block_id)
            .await?;

        self.batch_builder.recover_from(
            txs.to_vec(),
            anchor_block_id,
            anchor_block_timestamp_sec,
            block.header.timestamp,
            coinbase,
        )
    }

    pub async fn get_l1_anchor_block_offset_for_l2_block(
        &self,
        l2_block_height: u64,
    ) -> Result<u64, Error> {
        debug!(
            "get_anchor_block_offset: Checking L2 block {}",
            l2_block_height
        );
        let block = self
            .taiko
            .get_l2_block_by_number(l2_block_height, false)
            .await?;

        let anchor_tx_hash = block
            .transactions
            .as_hashes()
            .and_then(|txs| txs.first())
            .ok_or_else(|| anyhow::anyhow!("get_anchor_block_offset: No transactions in block"))?;

        let l2_anchor_tx = self.taiko.get_transaction_by_hash(*anchor_tx_hash).await?;
        let l1_anchor_block_id = Taiko::decode_anchor_tx_data(l2_anchor_tx.input())?;

        debug!(
            "get_l1_anchor_block_offset_for_l2_block: L2 block {l2_block_height} has L1 anchor block id {l1_anchor_block_id}"
        );

        self.ethereum_l1.slot_clock.slots_since_l1_block(
            self.ethereum_l1
                .execution_layer
                .get_block_timestamp_by_id(l1_anchor_block_id)
                .await?,
        )
    }

    pub fn is_anchor_block_offset_valid(&self, anchor_block_offset: u64) -> bool {
        anchor_block_offset + MIN_SLOTS_TO_PROPOSE
            < self
                .ethereum_l1
                .execution_layer
                .get_config_max_anchor_height_offset()
    }

    pub async fn reanchor_block(
        &mut self,
        pending_tx_list: PreBuiltTxList,
        l2_slot_info: L2SlotInfo,
    ) -> Result<Option<BuildPreconfBlockResponse>, Error> {
        let l2_block = L2Block::new_from(pending_tx_list, l2_slot_info.slot_timestamp());
        self.add_new_l2_block(l2_block, l2_slot_info, false, OperationType::Reanchor)
            .await
    }

    pub async fn preconfirm_block(
        &mut self,
        pending_tx_list: Option<PreBuiltTxList>,
        l2_slot_info: L2SlotInfo,
        end_of_sequencing: bool,
    ) -> Result<Option<BuildPreconfBlockResponse>, Error> {
        let result = if let Some(pending_tx_list) = pending_tx_list {
            // Handle the pending tx list from taiko geth
            debug!(
                "Received pending tx list length: {}, bytes length: {}",
                pending_tx_list.tx_list.len(),
                pending_tx_list.bytes_length
            );
            let l2_block = L2Block::new_from(pending_tx_list, l2_slot_info.slot_timestamp());
            self.add_new_l2_block(
                l2_block,
                l2_slot_info,
                end_of_sequencing,
                OperationType::Preconfirm,
            )
            .await?
        } else if self.is_empty_block_required(l2_slot_info.slot_timestamp()) {
            // Handle time shift between blocks exceeded
            debug!("No pending txs, proposing empty block");
            let empty_block = L2Block::new_empty(l2_slot_info.slot_timestamp());
            self.add_new_l2_block(
                empty_block,
                l2_slot_info,
                end_of_sequencing,
                OperationType::Preconfirm,
            )
            .await?
        } else if end_of_sequencing {
            debug!("No pending txs, but reached end of sequencing, proposing empty block.");
            let empty_block = L2Block::new_empty(l2_slot_info.slot_timestamp());
            self.add_new_l2_block(
                empty_block,
                l2_slot_info,
                end_of_sequencing,
                OperationType::Preconfirm,
            )
            .await?
        } else {
            trace!("No pending txs, skipping preconfirmation");
            None
        };

        if self
            .batch_builder
            .is_greater_than_max_anchor_height_offset()?
        {
            // Handle max anchor height offset exceeded
            info!("📈 Maximum allowed anchor height offset exceeded, finalizing current batch.");
            self.batch_builder.finalize_current_batch();
        }

        Ok(result)
    }

    async fn add_new_l2_block(
        &mut self,
        l2_block: L2Block,
        l2_slot_info: L2SlotInfo,
        end_of_sequencing: bool,
        operation_type: OperationType,
    ) -> Result<Option<BuildPreconfBlockResponse>, Error> {
        info!(
            "Adding new L2 block id: {}, timestamp: {}, parent gas used: {}",
            l2_slot_info.parent_id() + 1,
            l2_slot_info.slot_timestamp(),
            l2_slot_info.parent_gas_used()
        );
        let anchor_block_id: u64 = self.consume_l2_block(l2_block.clone()).await?;

        match self
            .taiko
            .advance_head_to_new_l2_block(
                l2_block,
                anchor_block_id,
                l2_slot_info,
                end_of_sequencing,
                operation_type,
            )
            .await
        {
            Ok(preconfed_block) => Ok(preconfed_block),
            Err(err) => {
                error!("Failed to advance head to new L2 block: {}", err);
                self.remove_last_l2_block();
                Ok(None)
            }
        }
    }

    pub async fn consume_l2_block(&mut self, l2_block: L2Block) -> Result<u64, Error> {
        // If the L2 block can be added to the current batch, do so
        let anchor_block_id = if self.batch_builder.can_consume_l2_block(&l2_block) {
            self.batch_builder
                .add_l2_block_and_get_current_anchor_block_id(l2_block)?
        } else {
            // Otherwise, calculate the anchor block ID and create a new batch
            let anchor_block_id = self.calculate_anchor_block_id().await?;
            let anchor_block_timestamp_sec = self
                .ethereum_l1
                .execution_layer
                .get_block_timestamp_by_id(anchor_block_id)
                .await?;
            // Add the L2 block to the new batch
            self.batch_builder.create_new_batch_and_add_l2_block(
                anchor_block_id,
                anchor_block_timestamp_sec,
                l2_block,
                None,
            );
            anchor_block_id
        };
        Ok(anchor_block_id)
    }

    fn remove_last_l2_block(&mut self) {
        self.batch_builder.remove_last_l2_block();
    }

    async fn calculate_anchor_block_id(&self) -> Result<u64, Error> {
        let height_from_last_batch = self
            .taiko
            .get_last_synced_anchor_block_id_from_taiko_anchor()
            .await?;
        let l1_height = self.ethereum_l1.execution_layer.get_l1_height().await?;
        let l1_height_with_lag = l1_height - self.l1_height_lag;
        let anchor_id_from_last_l2_block =
            match self.taiko.get_last_synced_anchor_block_id_from_geth().await {
                Ok(height) => height,
                Err(err) => {
                    warn!(
                        "Failed to get last anchor block ID from Taiko Geth: {:?}",
                        err
                    );
                    0
                }
            };

        Ok(std::cmp::max(
            std::cmp::max(height_from_last_batch, l1_height_with_lag),
            anchor_id_from_last_l2_block,
        ))
    }

    pub async fn try_submit_oldest_batch(
        &mut self,
        submit_only_full_batches: bool,
    ) -> Result<(), Error> {
        self.batch_builder
            .try_submit_oldest_batch(self.ethereum_l1.clone(), submit_only_full_batches)
            .await
    }

    pub fn is_empty_block_required(&self, preconfirmation_timestamp: u64) -> bool {
        self.batch_builder
            .is_time_shift_between_blocks_expiring(preconfirmation_timestamp)
    }

    pub fn has_batches(&self) -> bool {
        !self.batch_builder.is_empty()
    }

    pub fn get_number_of_batches(&self) -> u64 {
        self.batch_builder.get_number_of_batches()
    }

    pub fn get_number_of_batches_ready_to_send(&self) -> u64 {
        self.batch_builder.get_number_of_batches_ready_to_send()
    }

    pub fn reset_builder(&mut self) {
        warn!("Resetting batch builder");
        self.batch_builder = batch_builder::BatchBuilder::new(
            self.batch_builder.get_config().clone(),
            self.ethereum_l1.slot_clock.clone(),
        );
    }

    pub fn clone_without_batches(&self) -> Self {
        Self {
            batch_builder: self.batch_builder.clone_without_batches(),
            ethereum_l1: self.ethereum_l1.clone(),
            taiko: self.taiko.clone(),
            l1_height_lag: self.l1_height_lag,
        }
    }

    pub fn prepend_batches(&mut self, batches: VecDeque<Batch>) {
        self.batch_builder.prepend_batches(batches);
    }

    pub fn finalize_current_batch(&mut self) {
        self.batch_builder.finalize_current_batch();
    }

    pub fn take_batches_to_send(&mut self) -> VecDeque<Batch> {
        self.batch_builder.take_batches_to_send()
    }
}
