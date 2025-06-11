use alloy::primitives::U256;
use anyhow::Error;
use std::{sync::Arc, time::Duration};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::{ethereum_l1, metrics::Metrics, taiko::Taiko};

pub struct FundsMonitor {
    ethereum_l1: Arc<ethereum_l1::EthereumL1>,
    taiko: Arc<Taiko>,
    metrics: Arc<Metrics>,
    thresholds: Thresholds,
    amount_to_bridge_from_l2_to_l1: u128,
    cancel_token: CancellationToken,
}

const MONITOR_INTERVAL_SEC: u64 = 10; //TODO

pub struct Thresholds {
    pub eth: u128,
    pub taiko: u128,
}

impl FundsMonitor {
    pub fn new(
        ethereum_l1: Arc<ethereum_l1::EthereumL1>,
        taiko: Arc<Taiko>,
        metrics: Arc<Metrics>,
        thresholds: Thresholds,
        amount_to_bridge_from_l2_to_l1: u128,
        cancel_token: CancellationToken,
    ) -> Self {
        Self {
            ethereum_l1,
            taiko,
            metrics,
            thresholds,
            amount_to_bridge_from_l2_to_l1,
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
        if let Err(e) = self.check_initial_funds().await {
            error!("{}", e);
            self.cancel_token.cancel();
            return;
        }

        loop {
            self.transfer_funds_from_l2_to_l1_when_needed().await;
            tokio::select! {
                _ = sleep(Duration::from_secs(MONITOR_INTERVAL_SEC)) => {},
                _ = self.cancel_token.cancelled() => {
                    info!("Shutdown signal received, exiting metrics loop...");
                    return;
                }
            }
        }
    }

    async fn check_initial_funds(&self) -> Result<(), Error> {
        // Check TAIKO TOKEN balance
        let total_balance = self
            .ethereum_l1
            .execution_layer
            .get_preconfer_total_bonds()
            .await
            .map_err(|e| Error::msg(format!("Failed to fetch bond balance: {}", e)))?;

        if total_balance < U256::from(self.thresholds.taiko) {
            anyhow::bail!(
                "Total balance ({}) is below the required threshold ({})",
                total_balance,
                self.thresholds.taiko
            );
        }

        info!("Preconfer taiko balance are sufficient: {}", total_balance);

        // Check ETH balance
        let balance = self
            .ethereum_l1
            .execution_layer
            .get_preconfer_wallet_eth()
            .await
            .map_err(|e| Error::msg(format!("Failed to fetch ETH balance: {}", e)))?;

        if balance < U256::from(self.thresholds.eth) {
            anyhow::bail!(
                "ETH balance ({}) is below the required threshold ({})",
                balance,
                self.thresholds.eth
            );
        }

        info!("ETH balance is sufficient ({})", balance);

        Ok(())
    }

    async fn transfer_funds_from_l2_to_l1_when_needed(&self) {
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

        const AMOUNT_TO_TRANSFER: u64 = 10000000000000000;
        match self
            .taiko
            .transfer_eth_from_l2_to_l1(AMOUNT_TO_TRANSFER)
            .await
        {
            Ok(_) => info!("Transferred {} ETH from L2 to L1", AMOUNT_TO_TRANSFER),
            Err(e) => warn!("Failed to transfer ETH from L2 to L1: {}", e),
        }
    }
}
