use std::{sync::Arc, time::Duration};

use alloy::{primitives::B256, rpc::types::Transaction};
use anyhow::Error;

use crate::ethereum_l1::{EthereumL1, consensus_layer};

pub struct BlobParser {
    ethereum_l1: Arc<EthereumL1>,
}

impl BlobParser {
    pub fn new(ethereum_l1: Arc<EthereumL1>) -> Self {
        Self { ethereum_l1 }
    }

    pub async fn extract_transactions_from_blob(
        &self,
        block: u64,
        blob_hash: Vec<B256>,
        tx_list_offset: u32,
        tx_list_size: u32,
    ) -> Result<Vec<Transaction>, Error> {
        let timestamp = self
            .ethereum_l1
            .execution_layer
            .get_block_timestamp_by_number(block)
            .await?;
        let slot = self
            .ethereum_l1
            .slot_clock
            .slot_of(Duration::from_secs(timestamp))?;
        let sidecars = self
            .ethereum_l1
            .consensus_layer
            .get_blob_sidecars(slot)
            .await?;
        todo!();
    }
}
