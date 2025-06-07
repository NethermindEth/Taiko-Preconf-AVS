use alloy::primitives::Address;
use alloy::sol_types::SolEvent;
use anyhow::{Error, anyhow};

use std::collections::VecDeque;
use std::time::Duration;
use std::{str::FromStr, sync::Arc};
use tokio::sync::Mutex;
use tokio::sync::mpsc::{self, Receiver};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

use crate::ethereum_l1::l1_contracts_bindings::forced_inclusion_store::IForcedInclusionStore::{
    ForcedInclusion, ForcedInclusionConsumed, ForcedInclusionStored,
};
use crate::utils::event_listener::listen_for_event;

const MESSAGE_QUEUE_SIZE: usize = 20;
const SLEEP_DURATION: Duration = Duration::from_secs(15);

pub struct ForcedInclusionMonitor {
    ws_rpc_url: String,
    force_inclusion_store: Address,
    cancel_token: CancellationToken,
    queue: Arc<Mutex<VecDeque<ForcedInclusion>>>,
}

impl ForcedInclusionMonitor {
    pub fn new(
        ws_rpc_url: String,
        force_inclusion_store: String,
        cancel_token: CancellationToken,
    ) -> Result<Self, Error> {
        debug!(
            "Creating ForceInclusionMonitor (L1: {}, Store: {})",
            ws_rpc_url, force_inclusion_store
        );

        let force_inclusion_store = Address::from_str(&force_inclusion_store)
            .map_err(|e| anyhow!("Invalid ForceInclusionStore address: {:?}", e))?;

        Ok(Self {
            ws_rpc_url,
            force_inclusion_store,
            cancel_token,
            queue: Arc::new(Mutex::new(VecDeque::new())),
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
                    queue.lock().await.push_back(stored.forcedInclusion);
                }
                Some(consumed) = forced_inclusion_consumed_rx.recv() => {
                    info!(
                        "ForcedInclusionConsumed event → lastBlockId = {}",
                        consumed.forcedInclusion.blobCreatedIn
                    );
                    queue.lock().await.pop_front();
                }

            }
        }
    }
}
