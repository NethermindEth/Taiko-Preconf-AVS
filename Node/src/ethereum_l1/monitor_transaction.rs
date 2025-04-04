use crate::ethereum_l1::ws_provider::WsProvider;
use alloy::{
    consensus::{TxEip4844Variant, TxEnvelope},
    network::{Network, ReceiptResponse, TransactionBuilder, TransactionBuilder4844},
    primitives::{Address, TxKind, B256},
    providers::{
        ext::DebugApi, PendingTransactionBuilder, PendingTransactionError, Provider, WatchTxError,
    },
    rpc::types::{trace::geth::GethDebugTracingOptions, Transaction, TransactionRequest},
};
use alloy_json_rpc::RpcError;
use std::{sync::Arc, time::Duration};
use tokio::task::JoinHandle;
use tracing::{debug, error, info, warn};

// Transaction status enum
#[derive(Debug, Clone, PartialEq)]
pub enum TxStatus {
    Confirmed(u64), // Block number
    Failed(String), // Error message
    Pending,
}

#[derive(Debug, Clone)]
pub struct TransactionMonitor {
    provider: Arc<WsProvider>,
    min_priority_fee_per_gas_wei: u128,
    tx_fees_increase_percentage: u128,
    max_attempts_to_send_tx: u64,
    delay_between_tx_attempts: Duration,
}

impl TransactionMonitor {
    pub fn new(
        provider: Arc<WsProvider>,
        min_priority_fee_per_gas_wei: u64,
        tx_fees_increase_percentage: u64,
        max_attempts_to_send_tx: u64,
        delay_between_tx_attempts_sec: u64,
    ) -> Self {
        Self {
            provider,
            min_priority_fee_per_gas_wei: min_priority_fee_per_gas_wei as u128,
            tx_fees_increase_percentage: tx_fees_increase_percentage as u128,
            max_attempts_to_send_tx,
            delay_between_tx_attempts: Duration::from_secs(delay_between_tx_attempts_sec),
        }
    }

    /// Monitor a transaction until it is confirmed or fails.
    /// Spawns a new tokio task to monitor the transaction.
    pub fn monitor_new_transaction(
        &self,
        tx: TransactionRequest,
        nonce: u64,
    ) -> JoinHandle<TxStatus> {
        self.clone().spawn_monitoring_task(tx, nonce)
    }

    pub fn spawn_monitoring_task(self, tx: TransactionRequest, nonce: u64) -> JoinHandle<TxStatus> {
        tokio::spawn(async move {
            let mut tx_hash = B256::ZERO;

            if tx.max_fee_per_gas.is_none() || tx.max_priority_fee_per_gas.is_none() {
                warn!("Cannot modify fees of legacy transaction");
                match self.provider.send_transaction(tx).await {
                    Ok(pending_tx) => {
                        return self.check_tx_receipt(pending_tx).await;
                    }
                    Err(e) => {
                        error!("Failed to send transaction: {:?}", e);
                        return TxStatus::Failed(e.to_string());
                    }
                }
            }

            // gas fees are some
            let mut max_priority_fee_per_gas = tx.max_priority_fee_per_gas.unwrap();
            let mut max_fee_per_gas = tx.max_fee_per_gas.unwrap();
            let mut max_fee_per_blob_gas = tx.max_fee_per_blob_gas;

            for attempt in 0..self.max_attempts_to_send_tx {
                let mut tx_clone = tx.clone();
                if attempt > 0 {
                    // replacement requires 100% more for penalty
                    max_fee_per_gas += max_fee_per_gas;
                    max_priority_fee_per_gas += max_priority_fee_per_gas;
                    if let Some(max_fee_per_blob_gas) = &mut max_fee_per_blob_gas {
                        *max_fee_per_blob_gas += *max_fee_per_blob_gas;
                    }
                } else {
                    // increase fees by percentage
                    max_fee_per_gas += max_fee_per_gas * self.tx_fees_increase_percentage / 100;
                    max_priority_fee_per_gas +=
                        max_priority_fee_per_gas * self.tx_fees_increase_percentage / 100;
                    if let Some(max_fee_per_blob_gas) = &mut max_fee_per_blob_gas {
                        *max_fee_per_blob_gas +=
                            *max_fee_per_blob_gas * self.tx_fees_increase_percentage / 100;
                    }

                    if max_priority_fee_per_gas < self.min_priority_fee_per_gas_wei {
                        max_fee_per_gas +=
                            self.min_priority_fee_per_gas_wei - max_priority_fee_per_gas;
                        max_priority_fee_per_gas = self.min_priority_fee_per_gas_wei;
                    }
                }

                tx_clone.set_max_priority_fee_per_gas(max_priority_fee_per_gas);
                tx_clone.set_max_fee_per_gas(max_fee_per_gas);
                if let Some(max_fee_per_blob_gas) = max_fee_per_blob_gas {
                    tx_clone.set_max_fee_per_blob_gas(max_fee_per_blob_gas);
                }
                tx_clone.set_nonce(nonce.into());

                debug!("Sending transaction max_fee_per_gas: {:?}, max_priority_fee_per_gas: {:?}, max_fee_per_blob_gas: {:?}, gas limit: {:?}, nonce: {:?}", tx_clone.max_fee_per_gas, tx_clone.max_priority_fee_per_gas, tx_clone.max_fee_per_blob_gas, tx_clone.gas, tx_clone.nonce);

                let pending_tx = match self.provider.send_transaction(tx_clone).await {
                    Ok(tx) => tx,
                    Err(e) => {
                        if let RpcError::ErrorResp(err) = &e {
                            if err.message.contains("nonce too low") {
                                // the message is probably already included
                                let status = self.verify_tx_included(tx_hash).await;
                                match status {
                                    TxStatus::Pending => {
                                        warn!(
                                            "Transaction {} is pending, got error: {}",
                                            tx_hash, err
                                        );
                                        continue;
                                    }
                                    _ => {
                                        return status;
                                    }
                                }
                            }
                        }
                        error!("Failed to send transaction: {}", e);
                        return TxStatus::Failed(e.to_string());
                    }
                };

                tx_hash = *pending_tx.tx_hash();
                debug!("Transaction hash: {}", tx_hash);

                let tx_status = self.check_tx_receipt(pending_tx).await;
                match tx_status {
                    TxStatus::Pending => continue,
                    _ => return tx_status,
                }
            }

            let error_msg = format!(
                "Transaction {} not confirmed after {} attempts",
                tx_hash, self.max_attempts_to_send_tx
            );
            error!("{}", error_msg);
            TxStatus::Failed(error_msg)
        })
    }

    async fn verify_tx_included(&self, tx_hash: B256) -> TxStatus {
        let tx = self.provider.get_transaction_by_hash(tx_hash).await;
        match tx {
            Ok(Some(tx)) => {
                if let Some(block_number) = tx.block_number {
                    info!(
                        "✅ Transaction {} confirmed in block {}",
                        tx_hash, block_number
                    );
                    TxStatus::Confirmed(block_number)
                } else {
                    TxStatus::Pending
                }
            }
            _ => {
                let error_msg = format!(
                    "Transaction {} not found, probably already included, check previous hashes",
                    tx_hash
                );
                warn!("{}", error_msg);
                TxStatus::Failed(error_msg)
            }
        }
    }

    async fn check_tx_receipt<N: Network>(
        &self,
        pending_tx: PendingTransactionBuilder<N>,
    ) -> TxStatus {
        let tx_hash = *pending_tx.tx_hash();
        let receipt = pending_tx
            .with_timeout(Some(self.delay_between_tx_attempts))
            .get_receipt()
            .await;

        match receipt {
            Ok(receipt) => {
                if receipt.status() {
                    let block_number = if let Some(block_number) = receipt.block_number() {
                        block_number
                    } else {
                        warn!("Block number not found for transaction {}", tx_hash);
                        0
                    };

                    info!(
                        "✅ Transaction {} confirmed in block {}",
                        tx_hash, block_number
                    );
                    TxStatus::Confirmed(block_number)
                } else if let Some(block_number) = receipt.block_number() {
                    TxStatus::Failed(self.check_for_revert_reason(tx_hash, block_number).await)
                } else {
                    let error_msg =
                        format!("Transaction {tx_hash} failed, but block number not found");
                    error!("{}", error_msg);
                    TxStatus::Failed(error_msg)
                }
            }
            Err(e) => match e {
                PendingTransactionError::TxWatcher(WatchTxError::Timeout) => {
                    debug!("Transaction watcher timeout");
                    TxStatus::Pending
                }
                _ => {
                    error!("Error checking transaction {}: {}", tx_hash, e);
                    TxStatus::Pending
                }
            },
        }
    }

    async fn check_for_revert_reason(&self, tx_hash: B256, block_number: u64) -> String {
        let default_options = GethDebugTracingOptions::default();
        let trace = self
            .provider
            .debug_trace_transaction(tx_hash, default_options)
            .await;

        let trace_errors = if let Ok(trace) = trace {
            Self::find_errors_from_trace(&format!("{:?}", trace))
        } else {
            None
        };

        let tx_details = match self.provider.get_transaction_by_hash(tx_hash).await {
            Ok(Some(tx)) => tx,
            _ => {
                let error_msg = format!("Transaction {} failed", tx_hash);
                error!("{}", error_msg);
                return error_msg;
            }
        };

        let call_request = Self::get_tx_request_for_call(tx_details);
        let revert_reason = match self
            .provider
            .call(call_request)
            .block(block_number.into())
            .await
        {
            Err(e) => e.to_string(),
            Ok(ok) => format!("Unknown revert reason: {ok}"),
        };

        let mut error_msg = format!("Transaction {tx_hash} failed: {revert_reason}");
        if let Some(trace_errors) = trace_errors {
            error_msg.push_str(&trace_errors);
        }
        error!("{}", error_msg);
        error_msg
    }

    fn find_errors_from_trace(trace_str: &str) -> Option<String> {
        let mut start_pos = 0;
        let mut error_message = String::new();
        while let Some(error_start) = trace_str[start_pos..].find("error: Some(") {
            let absolute_pos = start_pos + error_start;
            if let Some(closing_paren) = trace_str[absolute_pos..].find(')') {
                let error_content = &trace_str[absolute_pos..absolute_pos + closing_paren + 1];
                if !error_message.is_empty() {
                    error_message.push_str(", ");
                }
                error_message.push_str(error_content);
                start_pos = absolute_pos + closing_paren + 1;
            } else {
                break;
            }
        }
        if !error_message.is_empty() {
            Some(format!(", errors from debug trace: {error_message}"))
        } else {
            None
        }
    }

    fn get_tx_request_for_call(tx_details: Transaction) -> TransactionRequest {
        match tx_details.inner.inner() {
            TxEnvelope::Eip1559(tx) => {
                let to = match tx.tx().to {
                    TxKind::Call(to) => to,
                    _ => Address::default(),
                };
                TransactionRequest::default()
                    .with_from(tx_details.inner.signer())
                    .with_to(to)
                    .with_input(tx.tx().input.clone())
                    .with_value(tx.tx().value)
                    .with_gas_limit(tx.tx().gas_limit)
                    .with_max_priority_fee_per_gas(tx.tx().max_priority_fee_per_gas)
                    .with_max_fee_per_gas(tx.tx().max_fee_per_gas)
            }
            TxEnvelope::Legacy(tx) => {
                let to = match tx.tx().to {
                    TxKind::Call(to) => to,
                    _ => Address::default(),
                };
                TransactionRequest::default()
                    .with_from(tx_details.inner.signer())
                    .with_to(to)
                    .with_input(tx.tx().input.clone())
                    .with_value(tx.tx().value)
                    .with_gas_limit(tx.tx().gas_limit)
            }
            TxEnvelope::Eip2930(tx) => {
                let to = match tx.tx().to {
                    TxKind::Call(to) => to,
                    _ => Address::default(),
                };
                TransactionRequest::default()
                    .with_from(tx_details.inner.signer())
                    .with_to(to)
                    .with_input(tx.tx().input.clone())
                    .with_value(tx.tx().value)
                    .with_gas_limit(tx.tx().gas_limit)
            }
            TxEnvelope::Eip4844(tx) => {
                let tx = tx.tx();
                match tx {
                    TxEip4844Variant::TxEip4844(tx) => TransactionRequest::default()
                        .with_from(tx_details.inner.signer())
                        .with_to(tx.to)
                        .with_input(tx.input.clone())
                        .with_value(tx.value)
                        .with_gas_limit(tx.gas_limit),
                    TxEip4844Variant::TxEip4844WithSidecar(tx) => TransactionRequest::default()
                        .with_from(tx_details.inner.signer())
                        .with_to(tx.tx().to)
                        .with_input(tx.tx().input.clone())
                        .with_value(tx.tx().value)
                        .with_gas_limit(tx.tx().gas_limit)
                        .with_blob_sidecar(tx.sidecar.clone()),
                }
            }
            _ => TransactionRequest::default(),
        }
    }
}
