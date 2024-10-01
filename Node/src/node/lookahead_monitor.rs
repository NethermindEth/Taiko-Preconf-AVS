// Special mode of the AVS node that monitors the lookahead availability in the contract
// and push the lookahead if it is required.
use crate::ethereum_l1::EthereumL1;
use anyhow::Error;
use std::sync::Arc;
use tokio::time::Duration;
use tracing::{debug, error, info};

pub struct LookaheadMonitor {
    ethereum_l1: Arc<EthereumL1>,
    l1_slot_duration_sec: u64,
}

impl LookaheadMonitor {
    pub fn new(ethereum_l1: Arc<EthereumL1>, l1_slot_duration_sec: u64) -> Self {
        Self {
            ethereum_l1,
            l1_slot_duration_sec,
        }
    }

    pub async fn start(self) {
        // start lookahead monitor loop
        let mut interval = tokio::time::interval(Duration::from_secs(self.l1_slot_duration_sec));
        loop {
            interval.tick().await;

            if let Err(err) = self.lookahead_monitor_step().await {
                error!("Failed to execute lookahead monitor step: {}", err);
            }
        }
    }

    async fn lookahead_monitor_step(&self) -> Result<(), Error> {
        info!(
            "Monitoring lookahead, slot: {}",
            self.ethereum_l1.slot_clock.get_current_slot()?
        );

        let next_epoch = self.ethereum_l1.slot_clock.get_current_epoch()? + 1;
        if self
            .ethereum_l1
            .execution_layer
            .is_lookahead_required()
            .await?
        {
            debug!("Lookahead is required, pushing it");
            let cl_lookahead = self
                .ethereum_l1
                .consensus_layer
                .get_lookahead(next_epoch)
                .await?;

            let lookahead_params = self
                .ethereum_l1
                .execution_layer
                .get_lookahead_params_for_epoch_using_cl_lookahead(next_epoch, &cl_lookahead)
                .await?;

            self.ethereum_l1
                .execution_layer
                .force_push_lookahead(lookahead_params)
                .await?;
        }
        Ok(())
    }
}
