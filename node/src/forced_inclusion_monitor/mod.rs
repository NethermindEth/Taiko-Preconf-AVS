use alloy::primitives::Address;
use alloy::rpc::types::Transaction;
use alloy::sol_types::SolEvent;
use anyhow::{Error, anyhow};
use tokio::task::JoinHandle;

use std::collections::VecDeque;
use std::time::Duration;
use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;
use tokio::sync::mpsc::{self, Receiver};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::ethereum_l1::EthereumL1;
use crate::ethereum_l1::l1_contracts_bindings::forced_inclusion_store::IForcedInclusionStore::{
    ForcedInclusion, ForcedInclusionConsumed, ForcedInclusionStored,
};

use crate::node::blob_parser::extract_transactions_from_blob;
use crate::utils::event_listener::listen_for_event;

const MESSAGE_QUEUE_SIZE: usize = 20;
const SLEEP_DURATION: Duration = Duration::from_secs(15);

struct ForcedInclusionData {
    index: usize,
    txs_list: Option<Vec<Transaction>>,
    blob_decoding_handle: Option<JoinHandle<()>>,
    blob_decoding_token: Option<CancellationToken>,
}

impl ForcedInclusionData {
    fn is_decoding_in_progress(&self) -> bool {
        self.blob_decoding_handle
            .as_ref()
            .is_some_and(|h| !h.is_finished())
    }

    fn is_data_ready(&self) -> bool {
        self.txs_list.is_some()
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

    async fn reset(&mut self) {
        self.cancel_current_task().await;
        self.index = 0;
        self.txs_list = None;
    }

    fn decode(
        &mut self,
        forced_inclusion: ForcedInclusion,
        ethereum_l1: Arc<EthereumL1>,
        next_forced_inclusion_data: Arc<Mutex<ForcedInclusionData>>,
    ) {
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
                    next_forced_inclusion_data.lock().await.txs_list = txs;
                    info!("Decoding complete.");
                } => {}
            }
        });
        self.blob_decoding_handle = Some(handle);
    }
}

pub struct ForcedInclusionMonitor {
    ws_rpc_url: String,
    force_inclusion_store: Address,
    cancel_token: CancellationToken,
    queue: Arc<Mutex<VecDeque<ForcedInclusion>>>,
    next_forced_inclusion_data: Arc<Mutex<ForcedInclusionData>>,
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

        let force_inclinclusion_data = ethereum_l1
            .execution_layer
            .get_forced_incusion_store_data()
            .await?;

        Ok(Self {
            ws_rpc_url,
            force_inclusion_store,
            cancel_token,
            queue: Arc::new(Mutex::new(force_inclinclusion_data)),
            next_forced_inclusion_data: Arc::new(Mutex::new(ForcedInclusionData {
                index: 0,
                txs_list: None,
                blob_decoding_handle: None,
                blob_decoding_token: None,
            })),
            ethereum_l1,
        })
    }

    /// Spawns the event listeners and the message handler.
    pub async fn start(&self) -> Result<(), Error> {
        debug!("Starting ReorgDetector");

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
            self.next_forced_inclusion_data.clone(),
            self.queue.clone(),
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
                SLEEP_DURATION,
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
                SLEEP_DURATION,
            )
            .await;
        });
    }

    async fn handle_incoming_messages(
        ethereum_l1: Arc<EthereumL1>,
        next_forced_inclusion_data: Arc<Mutex<ForcedInclusionData>>,
        queue: Arc<Mutex<VecDeque<ForcedInclusion>>>,
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
                    let mut next_forced_inclusion_data_lock = next_forced_inclusion_data.lock().await;
                    // start a new decoding thread
                    if !next_forced_inclusion_data_lock.is_data_ready() && !next_forced_inclusion_data_lock.is_decoding_in_progress() {
                        if next_forced_inclusion_data_lock.index != 0 || next_forced_inclusion_data_lock.txs_list.is_some() {
                            warn!("Unexpected store value at index {}", next_forced_inclusion_data_lock.index);
                            next_forced_inclusion_data_lock.index = 0;
                            next_forced_inclusion_data_lock.txs_list = None;
                        }

                        next_forced_inclusion_data_lock.decode(
                            stored.forcedInclusion.clone(),
                            ethereum_l1.clone(),
                            next_forced_inclusion_data.clone(),
                        );
                    }
                    queue.lock().await.push_back(stored.forcedInclusion);
                }
                Some(consumed) = forced_inclusion_consumed_rx.recv() => {
                    info!(
                        "ForcedInclusionConsumed event → lastBlockId = {}",
                        consumed.forcedInclusion.blobCreatedIn
                    );
                    if let Some(front) = queue.lock().await.pop_front() {
                        if front.blobCreatedIn != consumed.forcedInclusion.blobCreatedIn ||
                           front.createdAtBatchId != consumed.forcedInclusion.createdAtBatchId ||
                           front.feeInGwei != consumed.forcedInclusion.feeInGwei ||
                           front.blobByteOffset != consumed.forcedInclusion.blobByteOffset ||
                           front.blobByteSize != consumed.forcedInclusion.blobByteSize ||
                           front.blobHash != consumed.forcedInclusion.blobHash {
                            error!("Expected Consumed ForcedInclusion at block {}, got block {}", front.blobCreatedIn, consumed.forcedInclusion.blobCreatedIn);
                            cancel_token.cancel();
                        }
                    } else {
                        error!("Queue is empty, expected Consumed ForcedInclusion at block {}", consumed.forcedInclusion.blobCreatedIn);
                        cancel_token.cancel();
                    }
                    let mut next_forced_inclusion_data_lock = next_forced_inclusion_data.lock().await;
                    if next_forced_inclusion_data_lock.index == 0 {
                        next_forced_inclusion_data_lock.reset().await;
                        next_forced_inclusion_data_lock.decode(
                            consumed.forcedInclusion,
                            ethereum_l1.clone(),
                            next_forced_inclusion_data.clone(),);
                    } else {
                        next_forced_inclusion_data_lock.index -= 1;
                    }
                }
            }
        }
    }

    // TODO: remove
    #[allow(dead_code)]
    pub async fn get_next_forced_inclusion_data(&self) -> Option<Vec<Transaction>> {
        let mut next_forced_inclusion_data_lock = self.next_forced_inclusion_data.lock().await;
        if !next_forced_inclusion_data_lock.is_data_ready() {
            return None;
        }
        let result = next_forced_inclusion_data_lock.txs_list.clone();

        if next_forced_inclusion_data_lock.is_decoding_in_progress() {
            error!("Unexpected: ForcedInclusion decoding is still in progress");
            self.cancel_token.cancel();
        }

        let next_index = next_forced_inclusion_data_lock.index + 1;
        if let Some(force_inclusion) = self.queue.lock().await.get(next_index) {
            next_forced_inclusion_data_lock.txs_list = None;
            next_forced_inclusion_data_lock.decode(
                force_inclusion.clone(),
                self.ethereum_l1.clone(),
                self.next_forced_inclusion_data.clone(),
            )
        } else {
            next_forced_inclusion_data_lock.index = 0;
            next_forced_inclusion_data_lock.txs_list = None;
        }

        result
    }
}
