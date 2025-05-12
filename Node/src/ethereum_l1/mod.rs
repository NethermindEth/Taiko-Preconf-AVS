pub mod config;
pub mod consensus_layer;
pub mod execution_layer;
mod l1_contracts_bindings;
mod monitor_transaction;
mod propose_batch_builder;
pub mod slot_clock;
pub mod transaction_error;
pub mod ws_provider;

use anyhow::Error;
use config::EthereumL1Config;
use consensus_layer::ConsensusLayer;
use execution_layer::ExecutionLayer;
use slot_clock::SlotClock;
use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use transaction_error::TransactionError;

use crate::metrics::Metrics;

pub struct EthereumL1 {
    pub slot_clock: Arc<SlotClock>,
    pub _consensus_layer: ConsensusLayer,
    pub execution_layer: Arc<ExecutionLayer>,
}

impl EthereumL1 {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        config: EthereumL1Config,
        transaction_error_channel: Sender<TransactionError>,
        metrics: Arc<Metrics>,
    ) -> Result<Self, Error> {
        tracing::info!("Creating EthereumL1 instance");
        let consensus_layer = ConsensusLayer::new(&config.consensus_rpc_url)?;
        let genesis_details = consensus_layer.get_genesis_details().await?;
        let slot_clock = Arc::new(SlotClock::new(
            0u64,
            genesis_details.genesis_time,
            config.slot_duration_sec,
            config.slots_per_epoch,
            config.preconf_heartbeat_ms,
        ));

        let execution_layer =
            ExecutionLayer::new(config, transaction_error_channel, metrics).await?;

        Ok(Self {
            slot_clock,
            _consensus_layer: consensus_layer,
            execution_layer: Arc::new(execution_layer),
        })
    }
}
