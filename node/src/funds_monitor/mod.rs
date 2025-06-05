use std::{sync::Arc, time::Duration};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::{ethereum_l1, metrics::Metrics, taiko::Taiko};

pub struct FundsMonitor {
    ethereum_l1: Arc<ethereum_l1::EthereumL1>,
    taiko: Arc<Taiko>,
    metrics: Arc<Metrics>,
    cancel_token: CancellationToken,
}

const MONITOR_INTERVAL_SEC: u64 = 10;  //TODO

impl FundsMonitor {
    pub fn new(
        ethereum_l1: Arc<ethereum_l1::EthereumL1>,
        taiko: Arc<Taiko>,
        metrics: Arc<Metrics>,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            ethereum_l1,
            taiko,
            metrics,
            cancel_token,
        }
    }

    pub fn run(self) {
        tokio::spawn(async move {
            info!("Starting funds monitor...");
            self.monitor_funds_level().await;
        });
    }

    async fn monitor_funds_level(self) {
        loop {
            let eth_balance = match self
                .ethereum_l1
                .execution_layer
                .get_preconfer_wallet_eth()
                .await
            {
                Ok(balance) => {
                    self.metrics.set_preconfer_eth_balance(balance);
                    format!("{}", balance)
                }
                Err(e) => {
                    warn!("Failed to get preconfer eth balance: {}", e);
                    "-".to_string()
                }
            };
            let taiko_balance = match self
                .ethereum_l1
                .execution_layer
                .get_preconfer_total_bonds()
                .await
            {
                Ok(balance) => {
                    self.metrics.set_preconfer_taiko_balance(balance);
                    format!("{}", balance)
                }
                Err(e) => {
                    warn!("Failed to get preconfer taiko balance: {}", e);
                    "-".to_string()
                }
            };

            let preconfer_address = self
                .ethereum_l1
                .execution_layer
                .get_preconfer_alloy_address();

            let l2_eth_balance = match self.taiko.get_balance(preconfer_address).await {
                Ok(balance) => {
                    self.metrics.set_preconfer_l2_eth_balance(balance);
                    format!("{}", balance)
                }
                Err(e) => {
                    warn!("Failed to get preconfer l2 eth balance: {}", e);
                    "-".to_string()
                }
            };

            info!(
                "Balances - ETH: {}, L2 ETH: {}, TAIKO: {}",
                eth_balance, l2_eth_balance, taiko_balance
            );

            match self.taiko.transfer_eth_from_l2_to_l1(1000000).await {
                Ok(_) => info!("Transferred 1000000 ETH from L2 to L1"),
                Err(e) => warn!("Failed to transfer ETH from L2 to L1: {}", e),
            }

            tokio::select! {
                _ = sleep(Duration::from_secs(MONITOR_INTERVAL_SEC)) => {},
                _ = self.cancel_token.cancelled() => {
                    info!("Shutdown signal received, exiting metrics loop...");
                    return;
                }
            }
        }
    }
}
