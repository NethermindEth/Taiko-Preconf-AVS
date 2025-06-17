use alloy::primitives::Address;
use alloy::rpc::types::Transaction;
use alloy::sol_types::SolEvent;
use anyhow::{Error, anyhow};

use std::time::Duration;
use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;
use tokio::sync::mpsc::{self, Receiver};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info};

use crate::ethereum_l1::EthereumL1;
use crate::ethereum_l1::l1_contracts_bindings::forced_inclusion_store::IForcedInclusionStore::{
    ForcedInclusionConsumed, ForcedInclusionStored,
};

mod forced_inclusion_data;
use forced_inclusion_data::ForcedInclusionData;

use crate::utils::event_listener::listen_for_event;

const MESSAGE_QUEUE_SIZE: usize = 20;
const RECONNECTION_DELAY: Duration = Duration::from_secs(15);

pub struct ForcedInclusionInfo {
    pub blob_hash: alloy::primitives::B256,
    pub blob_byte_offset: u32,
    pub blob_byte_size: u32,
    pub created_in: u64,
    pub txs: Vec<Transaction>,
}

pub struct ForcedInclusionMonitor {
    ws_rpc_url: String,
    force_inclusion_store: Address,
    cancel_token: CancellationToken,
    forced_inclusion_data: Arc<Mutex<ForcedInclusionData>>,
    ethereum_l1: Arc<EthereumL1>,
}

impl ForcedInclusionMonitor {
    pub async fn new(
        ws_rpc_url: String,
        force_inclusion_store: String,
        cancel_token: CancellationToken,
        ethereum_l1: Arc<EthereumL1>,
    ) -> Result<Self, Error> {
        debug!(
            "Creating ForceInclusionMonitor (L1: {}, Store: {})",
            ws_rpc_url, force_inclusion_store
        );

        let force_inclusion_store = Address::from_str(&force_inclusion_store)
            .map_err(|e| anyhow!("Invalid ForceInclusionStore address: {:?}", e))?;

        let queue = ethereum_l1
            .execution_layer
            .get_forced_incusion_store_data()
            .await?;

        let forced_inclusion_data = Arc::new(Mutex::new(ForcedInclusionData {
            index: 0,
            txs_list: None,
            blob_decoding_handle: None,
            blob_decoding_token: None,
            queue,
        }));

        forced_inclusion_data
            .lock()
            .await
            .try_decode(ethereum_l1.clone(), forced_inclusion_data.clone());

        Ok(Self {
            ws_rpc_url,
            force_inclusion_store,
            cancel_token,
            forced_inclusion_data,
            ethereum_l1,
        })
    }

    /// Spawns the event listeners and the message handler.
    pub async fn start(&self) -> Result<(), Error> {
        //ForcedInclusionStored events
        let (forced_inclusion_stored_tx, forced_inclusion_stored_rx) =
            mpsc::channel(MESSAGE_QUEUE_SIZE);
        self.spawn_forced_inclusion_stored_listener(forced_inclusion_stored_tx);

        // ForcedInclusionConsumed events
        let (forced_inclusion_consumed_tx, forced_inclusion_consumed_rx) =
            mpsc::channel(MESSAGE_QUEUE_SIZE);
        self.spawn_forced_inclusion_consumed_listener(forced_inclusion_consumed_tx);

        //Message dispatcher
        tokio::spawn(Self::handle_incoming_messages(
            self.ethereum_l1.clone(),
            self.forced_inclusion_data.clone(),
            forced_inclusion_stored_rx,
            forced_inclusion_consumed_rx,
            self.cancel_token.clone(),
        ));

        Ok(())
    }

    fn spawn_forced_inclusion_stored_listener(
        &self,
        forced_inclusion_stored_tx: mpsc::Sender<ForcedInclusionStored>,
    ) {
        info!("Starting ForcedInclusionStored event receiver");
        let ws_rpc_url = self.ws_rpc_url.clone();
        let force_inclusion_store = self.force_inclusion_store;
        let forced_inclusion_stored_tx = forced_inclusion_stored_tx.clone();
        let cancel_token = self.cancel_token.clone();

        tokio::spawn(async move {
            listen_for_event(
                ws_rpc_url,
                force_inclusion_store,
                "ForcedInclusionStored",
                ForcedInclusionStored::SIGNATURE_HASH,
                |log| Ok(ForcedInclusionStored::decode_log(&log.inner)?.data),
                forced_inclusion_stored_tx,
                cancel_token,
                RECONNECTION_DELAY,
            )
            .await;
        });
    }

    fn spawn_forced_inclusion_consumed_listener(
        &self,
        forced_inclusion_consumed_tx: mpsc::Sender<ForcedInclusionConsumed>,
    ) {
        info!("Starting ForcedInclusionConsumed event receiver");
        let ws_rpc_url = self.ws_rpc_url.clone();
        let force_inclusion_store = self.force_inclusion_store;
        let forced_inclusion_consumed_tx = forced_inclusion_consumed_tx.clone();
        let cancel_token = self.cancel_token.clone();

        tokio::spawn(async move {
            listen_for_event(
                ws_rpc_url,
                force_inclusion_store,
                "ForcedInclusionConsumed",
                ForcedInclusionConsumed::SIGNATURE_HASH,
                |log| Ok(ForcedInclusionConsumed::decode_log(&log.inner)?.data),
                forced_inclusion_consumed_tx,
                cancel_token,
                RECONNECTION_DELAY,
            )
            .await;
        });
    }

    async fn handle_incoming_messages(
        ethereum_l1: Arc<EthereumL1>,
        forced_inclusion_data: Arc<Mutex<ForcedInclusionData>>,
        mut forced_inclusion_stored_rx: Receiver<ForcedInclusionStored>,
        mut forced_inclusion_consumed_rx: Receiver<ForcedInclusionConsumed>,
        cancel_token: CancellationToken,
    ) {
        info!("message loop running");

        loop {
            tokio::select! {
                _ = cancel_token.cancelled() => {
                    info!("ForceInclusionMonitor: cancellation received, shutting down message loop");
                    break;
                }
                Some(stored) = forced_inclusion_stored_rx.recv() => {
                    info!(
                        "ForcedInclusionStored event → lastBlockId = {}",
                        stored.forcedInclusion.blobCreatedIn
                    );
                    let mut next_forced_inclusion_data_lock = forced_inclusion_data.lock().await;
                    debug!("Decoding ForcedInclusion at block {}", stored.forcedInclusion.blobCreatedIn);
                    next_forced_inclusion_data_lock.queue.push_back(stored.forcedInclusion);
                    if !next_forced_inclusion_data_lock.is_data_ready()
                        && !next_forced_inclusion_data_lock.is_decoding_in_progress()
                        && next_forced_inclusion_data_lock.index + 1 == next_forced_inclusion_data_lock.queue.len()
                    {
                        next_forced_inclusion_data_lock.try_decode(
                            ethereum_l1.clone(),
                            forced_inclusion_data.clone(),
                        );
                    }

                }
                Some(consumed) = forced_inclusion_consumed_rx.recv() => {
                    info!(
                        "ForcedInclusionConsumed event → lastBlockId = {}",
                        consumed.forcedInclusion.blobCreatedIn
                    );
                    let mut next_forced_inclusion_data_lock = forced_inclusion_data.lock().await;
                    if let Some(front) = next_forced_inclusion_data_lock.queue.pop_front() {
                        if front != consumed.forcedInclusion {
                            error!("Expected Consumed ForcedInclusion at block {}, got block {}", front.blobCreatedIn, consumed.forcedInclusion.blobCreatedIn);
                            cancel_token.cancel();
                        }
                    } else {
                        error!("Queue is empty, expected Consumed ForcedInclusion at block {}", consumed.forcedInclusion.blobCreatedIn);
                        cancel_token.cancel();
                    }
                    if next_forced_inclusion_data_lock.index == 0 {
                        next_forced_inclusion_data_lock.reset().await;
                        next_forced_inclusion_data_lock.try_decode(
                            ethereum_l1.clone(),
                            forced_inclusion_data.clone());
                    } else {
                        debug!("reduce index from {} to {}", next_forced_inclusion_data_lock.index, next_forced_inclusion_data_lock.index - 1);
                        next_forced_inclusion_data_lock.index -= 1;
                    }
                }
            }
        }
    }

    pub async fn is_same_txs_list(&self, input: &Vec<Transaction>) -> Option<bool> {
        let next_forced_inclusion_data_lock = self.forced_inclusion_data.lock().await;
        if !next_forced_inclusion_data_lock.is_data_exist() {
            debug!("next_forced_inclusion_data does not exist");
            return Some(false);
        }

        if !next_forced_inclusion_data_lock.is_data_ready() {
            debug!("next_forced_inclusion_data is not ready");
            return None;
        }

        if next_forced_inclusion_data_lock.is_decoding_in_progress() {
            error!("Unexpected: ForcedInclusion decoding is still in progress but data is ready");
            self.cancel_token.cancel();
            return Some(false);
        }

        if next_forced_inclusion_data_lock.txs_list.is_none() {
            error!("Unexpected: No transactions list found");
            self.cancel_token.cancel();
            return Some(false);
        }

        if let Some(txs) = &next_forced_inclusion_data_lock.txs_list {
            if txs == input {
                return Some(true);
            } else {
                return Some(false);
            }
        }
        None
    }

    pub async fn get_next_forced_inclusion_data(&self) -> Option<ForcedInclusionInfo> {
        let mut next_forced_inclusion_data_lock = self.forced_inclusion_data.lock().await;
        if !next_forced_inclusion_data_lock.is_data_exist() {
            debug!("next_forced_inclusion_data does not exist");
            return None;
        }

        if !next_forced_inclusion_data_lock.is_data_ready() {
            debug!("next_forced_inclusion_data is not ready");
            return None;
        }

        if next_forced_inclusion_data_lock.is_decoding_in_progress() {
            error!("Unexpected: ForcedInclusion decoding is still in progress but data is ready");
            self.cancel_token.cancel();
            return None;
        }

        let txs = match next_forced_inclusion_data_lock.txs_list.take() {
            Some(txs) => txs,
            None => {
                error!("Unexpected: No transactions found. skipping forced inclusion");
                self.cancel_token.cancel();
                return None;
            }
        };

        let result = match next_forced_inclusion_data_lock
            .queue
            .get(next_forced_inclusion_data_lock.index)
        {
            Some(forced_inclusion) => {
                let forced_inclusion = forced_inclusion.clone();
                Some(ForcedInclusionInfo {
                    blob_hash: forced_inclusion.blobHash,
                    blob_byte_offset: forced_inclusion.blobByteOffset,
                    blob_byte_size: forced_inclusion.blobByteSize,
                    created_in: forced_inclusion.blobCreatedIn,
                    txs,
                })
            }
            None => None,
        };

        next_forced_inclusion_data_lock.index += 1;
        debug!(
            "next_forced_inclusion_data index: {}",
            next_forced_inclusion_data_lock.index
        );
        next_forced_inclusion_data_lock
            .try_decode(self.ethereum_l1.clone(), self.forced_inclusion_data.clone());

        result
    }
}
