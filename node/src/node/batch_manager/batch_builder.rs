use std::{collections::VecDeque, sync::Arc};

use crate::{
    ethereum_l1::{EthereumL1, slot_clock::SlotClock, transaction_result::TransactionResult},
    shared::l2_block::L2Block,
};
use alloy::primitives::Address;
use anyhow::Error;
use tracing::{debug, trace, warn};

use super::BatchBuilderConfig;

#[derive(Default)]
pub struct Batch {
    pub l2_blocks: Vec<L2Block>,
    pub total_bytes: u64,
    pub coinbase: Address,
    pub anchor_block_id: u64,
    pub anchor_block_timestamp_sec: u64,
}

pub struct BatchBuilder {
    config: BatchBuilderConfig,
    batches_to_send: VecDeque<Batch>,
    current_batch: Option<Batch>,
    slot_clock: Arc<SlotClock>,
}

impl Drop for BatchBuilder {
    fn drop(&mut self) {
        debug!(
            "BatchBuilder dropped! current_batch is none: {}, batches_to_send len: {}",
            self.current_batch.is_none(),
            self.batches_to_send.len()
        );
    }
}

impl BatchBuilder {
    pub fn new(config: BatchBuilderConfig, slot_clock: Arc<SlotClock>) -> Self {
        Self {
            config,
            batches_to_send: VecDeque::new(),
            current_batch: None,
            slot_clock,
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
            .is_some_and(|batch| {
                let number_of_blocks = if let Ok(n) = u16::try_from(batch.l2_blocks.len() + 1) {n} else {
            return false;
        };
                // Check if the total bytes of the current batch after adding the new L2 block
                // is less than or equal to the max bytes size of the batch
                self.config.is_within_bytes_limit(batch.total_bytes + l2_block.prebuilt_tx_list.bytes_length)
                    // Check if the number of L2 blocks in the current batch after adding the new L2 block
                    // is less than or equal to the max blocks per batch
                    && self.config.is_within_block_limit(number_of_blocks)
                    // check that time shift between blocks is not expired
                    && !self.is_time_shift_expired(l2_block.timestamp_sec)
            })
    }

    pub fn finalize_current_batch(&mut self) {
        if let Some(batch) = self.current_batch.take() {
            self.batches_to_send.push_back(batch);
        }
    }

    pub fn create_new_batch_and_add_l2_block(
        &mut self,
        anchor_block_id: u64,
        anchor_block_timestamp_sec: u64,
        l2_block: L2Block,
        coinbase: Option<Address>,
    ) {
        self.finalize_current_batch();
        self.current_batch = Some(Batch {
            total_bytes: l2_block.prebuilt_tx_list.bytes_length,
            l2_blocks: vec![l2_block],
            anchor_block_id,
            anchor_block_timestamp_sec,
            coinbase: coinbase.unwrap_or(self.config.default_coinbase),
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
                "Added L2 block to batch: l2 blocks: {}, total bytes: {}",
                current_batch.l2_blocks.len(),
                current_batch.total_bytes
            );
            Ok(current_batch.anchor_block_id)
        } else {
            Err(anyhow::anyhow!("No current batch"))
        }
    }

    pub fn remove_last_l2_block(&mut self) {
        if let Some(current_batch) = self.current_batch.as_mut() {
            let removed_block = current_batch.l2_blocks.pop();
            if let Some(removed_block) = removed_block {
                current_batch.total_bytes -= removed_block.prebuilt_tx_list.bytes_length;
                if current_batch.l2_blocks.is_empty() {
                    self.current_batch = None;
                }
                debug!(
                    "Removed L2 block from batch: {} txs, {} bytes",
                    removed_block.prebuilt_tx_list.tx_list.len(),
                    removed_block.prebuilt_tx_list.bytes_length
                );
            }
        }
    }

    pub fn recover_from(
        &mut self,
        tx_list: Vec<alloy::rpc::types::Transaction>,
        anchor_block_id: u64,
        anchor_block_timestamp_sec: u64,
        l2_block_timestamp_sec: u64,
        coinbase: Address,
    ) -> Result<(), Error> {
        // We have a new batch if any of the following is true:
        // 1. Anchor block IDs differ
        // 2. Time difference between two blocks exceeds u8
        if !self.is_same_anchor_block_id(anchor_block_id)
            || self.is_time_shift_expired(l2_block_timestamp_sec)
            || !self.is_same_coinbase(coinbase)
        {
            self.finalize_current_batch();
            self.current_batch = Some(Batch {
                total_bytes: 0,
                l2_blocks: vec![],
                anchor_block_id,
                coinbase,
                anchor_block_timestamp_sec,
            });
        }

        let bytes_length = crate::shared::l2_tx_lists::encode_and_compress(&tx_list)?.len() as u64;
        let l2_block = L2Block::new_from(
            crate::shared::l2_tx_lists::PreBuiltTxList {
                tx_list,
                estimated_gas_used: 0,
                bytes_length,
            },
            l2_block_timestamp_sec,
        );

        if self.can_consume_l2_block(&l2_block) {
            self.add_l2_block_and_get_current_anchor_block_id(l2_block)?;
        } else {
            self.create_new_batch_and_add_l2_block(
                anchor_block_id,
                anchor_block_timestamp_sec,
                l2_block,
                Some(coinbase),
            );
        }

        Ok(())
    }

    fn is_same_anchor_block_id(&self, anchor_block_id: u64) -> bool {
        self.current_batch
            .as_ref()
            .is_some_and(|batch| batch.anchor_block_id == anchor_block_id)
    }

    fn is_same_coinbase(&self, coinbase: Address) -> bool {
        self.current_batch
            .as_ref()
            .is_some_and(|batch| batch.coinbase == coinbase)
    }

    pub fn is_empty(&self) -> bool {
        trace!(
            "batch_builder::is_empty: current_batch is none: {}, batches_to_send len: {}",
            self.current_batch.is_none(),
            self.batches_to_send.len()
        );
        self.current_batch.is_none() && self.batches_to_send.is_empty()
    }

    pub async fn try_submit_oldest_batch(
        &mut self,
        ethereum_l1: Arc<EthereumL1>,
        submit_only_full_batches: bool,
    ) -> Result<(), Error> {
        if self.current_batch.is_some()
            && (!submit_only_full_batches
                || !self.config.is_within_block_limit(
                    u16::try_from(
                        self.current_batch
                            .as_ref()
                            .expect("assert: current batch is available")
                            .l2_blocks
                            .len(),
                    )? + 1,
                ))
        {
            self.finalize_current_batch();
        }

        if let Some(batch) = self.batches_to_send.front() {
            if ethereum_l1
                .execution_layer
                .is_transaction_in_progress()
                .await?
            {
                debug!(
                    batches_to_send = %self.batches_to_send.len(),
                    current_batch = %self.current_batch.is_some(),
                    "Cannot submit batch, transaction is in progress.",
                );
                return Ok(());
            }

            debug!(
                anchor_block_id = %batch.anchor_block_id,
                coinbase = %batch.coinbase,
                l2_blocks_len = %batch.l2_blocks.len(),
                total_bytes = %batch.total_bytes,
                batches_to_send = %self.batches_to_send.len(),
                current_batch = %self.current_batch.is_some(),
                "Submitting batch"
            );

            if let Err(err) = ethereum_l1
                .execution_layer
                .send_batch_to_l1(
                    batch.l2_blocks.clone(),
                    batch.anchor_block_id,
                    batch.coinbase,
                    self.slot_clock.get_current_slot_begin_timestamp()?,
                )
                .await
            {
                if let Some(transaction_result) = err.downcast_ref::<TransactionResult>() {
                    if !matches!(transaction_result, TransactionResult::EstimationTooEarly) {
                        debug!("BatchBuilder: Transaction error, removing all batches");
                        self.batches_to_send.clear();
                    }
                }
                return Err(err);
            }
        }

        Ok(())
    }

    pub fn pop_front_batch(&mut self) -> Option<Batch> {
        self.batches_to_send.pop_front()
    }

    pub fn is_time_shift_expired(&self, current_l2_slot_timestamp: u64) -> bool {
        if let Some(current_batch) = self.current_batch.as_ref() {
            if let Some(last_block) = current_batch.l2_blocks.last() {
                return current_l2_slot_timestamp - last_block.timestamp_sec
                    > self.config.max_time_shift_between_blocks_sec;
            }
        }
        false
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

    pub fn is_greater_than_max_anchor_height_offset(&self) -> Result<bool, Error> {
        if let Some(current_batch) = self.current_batch.as_ref() {
            let slots_since_l1_block = self
                .slot_clock
                .slots_since_l1_block(current_batch.anchor_block_timestamp_sec)?;
            return Ok(slots_since_l1_block > self.config.max_anchor_height_offset);
        }
        Ok(false)
    }

    pub fn clone_without_batches(&self) -> Self {
        Self {
            config: self.config.clone(),
            batches_to_send: VecDeque::new(),
            current_batch: None,
            slot_clock: self.slot_clock.clone(),
        }
    }

    pub fn get_number_of_batches(&self) -> u64 {
        self.batches_to_send.len() as u64 + if self.current_batch.is_some() { 1 } else { 0 }
    }

    pub fn get_number_of_batches_ready_to_send(&self) -> u64 {
        self.batches_to_send.len() as u64
    }

    pub fn take_batches_to_send(&mut self) -> VecDeque<Batch> {
        std::mem::take(&mut self.batches_to_send)
    }

    pub fn prepend_batches(&mut self, mut batches: VecDeque<Batch>) {
        batches.append(&mut self.batches_to_send);
        self.batches_to_send = batches;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_the_last_l1_slot_to_add_an_empty_l2_block() {
        let batch_builder = BatchBuilder::new(
            BatchBuilderConfig {
                max_bytes_size_of_batch: 1000,
                max_blocks_per_batch: 10,
                l1_slot_duration_sec: 12,
                max_time_shift_between_blocks_sec: 255,
                max_anchor_height_offset: 10,
                default_coinbase: Address::ZERO,
            },
            Arc::new(SlotClock::new(0, 5, 12, 32, 3000)),
        );

        assert!(!batch_builder.is_the_last_l1_slot_to_add_an_empty_l2_block(100, 0));
        assert!(!batch_builder.is_the_last_l1_slot_to_add_an_empty_l2_block(242, 0));
        assert!(batch_builder.is_the_last_l1_slot_to_add_an_empty_l2_block(243, 0));
        assert!(batch_builder.is_the_last_l1_slot_to_add_an_empty_l2_block(255, 0));
    }
}
