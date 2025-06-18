use alloy::rpc::types::Transaction;
use tokio::task::JoinHandle;

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::ethereum_l1::EthereumL1;
use crate::ethereum_l1::l1_contracts_bindings::forced_inclusion_store::IForcedInclusionStore::ForcedInclusion;

use crate::node::blob_parser::extract_transactions_from_blob;

pub struct ForcedInclusionData {
    pub index: usize,
    pub txs_list: Option<Vec<Transaction>>,
    pub blob_decoding_handle: Option<JoinHandle<()>>,
    pub blob_decoding_token: Option<CancellationToken>,
    pub queue: VecDeque<ForcedInclusion>,
}

impl ForcedInclusionData {
    pub fn is_decoding_in_progress(&self) -> bool {
        self.blob_decoding_handle
            .as_ref()
            .is_some_and(|h| !h.is_finished())
    }

    pub fn is_data_ready(&self) -> bool {
        self.txs_list.is_some()
    }

    pub fn is_data_exist(&self) -> bool {
        self.index < self.queue.len()
    }

    async fn cancel_current_task(&mut self) {
        if let Some(blob_decoding_handle) = self.blob_decoding_handle.take() {
            if let Some(blob_decoding_token) = self.blob_decoding_token.take() {
                debug!("Cancelling blob decoding task for index {}", self.index);
                blob_decoding_token.cancel();
                while !blob_decoding_handle.is_finished() {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                self.blob_decoding_handle = None;
                self.blob_decoding_token = None;
                debug!("Blob decoding task cancelled");
            }
        }
    }

    pub async fn reset(&mut self) {
        self.cancel_current_task().await;
        self.index = 0;
        self.txs_list = None;
    }

    pub fn try_decode(
        &mut self,
        ethereum_l1: Arc<EthereumL1>,
        next_forced_inclusion_data: Arc<Mutex<ForcedInclusionData>>,
    ) -> bool {
        if self.is_decoding_in_progress() {
            warn!("ForcedInclusion decoding is already in progress");
            return false;
        }

        let forced_inclusion = match self.queue.get(self.index) {
            Some(forced_inclusion) => forced_inclusion.clone(),
            None => {
                debug!(
                    "No forced_inclusion at index {} length {}",
                    self.index,
                    self.queue.len()
                );
                return false;
            }
        };

        let decoding_token = CancellationToken::new();
        self.blob_decoding_token = Some(decoding_token.clone());

        let handle = tokio::spawn(async move {
            // Replace with your real decoding logic
            tokio::select! {
                _ = decoding_token.cancelled() => {
                    info!("decoding task was cancelled.");
                }
                _ = async {
                    info!("Decoding new ForcedInclusion...");
                    let txs = match extract_transactions_from_blob(
                        ethereum_l1,
                        forced_inclusion.blobCreatedIn,
                        [forced_inclusion.blobHash].to_vec(),
                        forced_inclusion.blobByteOffset,
                        forced_inclusion.blobByteSize
                    ).await {
                        Ok(txs) => Some(txs),
                        Err(e) => {
                            error!("Error decoding ForcedInclusion: {}", e);
                            None
                        }
                    };
                    debug!("Decoded txs {:?}", txs);
                    next_forced_inclusion_data.lock().await.txs_list = txs;
                    info!("Decoding complete.");
                } => {}
            }
        });
        self.blob_decoding_handle = Some(handle);

        true
    }
}
