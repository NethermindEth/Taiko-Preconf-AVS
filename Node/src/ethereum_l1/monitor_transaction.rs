use crate::metrics::Metrics;

use super::{transaction_error::TransactionError, ws_provider::WsProvider};
use alloy::{
    consensus::{TxEip4844Variant, TxEnvelope, TxType},
    network::{Network, ReceiptResponse, TransactionBuilder, TransactionBuilder4844},
    primitives::{Address, TxKind, B256},
    providers::{
        ext::DebugApi, PendingTransactionBuilder, PendingTransactionError, Provider, RootProvider,
        WatchTxError,
    },
    rpc::types::{trace::geth::GethDebugTracingOptions, Transaction, TransactionRequest},
};
use alloy_json_rpc::RpcError;
use anyhow::Error;
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;
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
pub struct TransactionMonitorConfig {
    min_priority_fee_per_gas_wei: u128,
    tx_fees_increase_percentage: u128,
    max_attempts_to_send_tx: u64,
    delay_between_tx_attempts: Duration,
}

pub struct TransactionMonitorThread {
    provider: Arc<WsProvider>,
    config: TransactionMonitorConfig,
    nonce: u64,
    error_notification_channel: Sender<TransactionError>,
    metrics: Arc<Metrics>,
}

//#[derive(Debug)]
pub struct TransactionMonitor {
    provider: Arc<WsProvider>,
    config: TransactionMonitorConfig,
    join_handle: Mutex<Option<JoinHandle<()>>>,
    error_notification_channel: Sender<TransactionError>,
    metrics: Arc<Metrics>,
}

impl TransactionMonitor {
    pub async fn new(
        provider: Arc<WsProvider>,
        min_priority_fee_per_gas_wei: u64,
        tx_fees_increase_percentage: u64,
        max_attempts_to_send_tx: u64,
        delay_between_tx_attempts_sec: u64,
        error_notification_channel: Sender<TransactionError>,
        metrics: Arc<Metrics>,
    ) -> Result<Self, Error> {
        Ok(Self {
            provider,
            config: TransactionMonitorConfig {
                min_priority_fee_per_gas_wei: min_priority_fee_per_gas_wei as u128,
                tx_fees_increase_percentage: tx_fees_increase_percentage as u128,
                max_attempts_to_send_tx,
                delay_between_tx_attempts: Duration::from_secs(delay_between_tx_attempts_sec),
            },
            join_handle: Mutex::new(None),
            error_notification_channel,
            metrics,
        })
    }

    /// Monitor a transaction until it is confirmed or fails.
    /// Spawns a new tokio task to monitor the transaction.
    pub async fn monitor_new_transaction(
        &self,
        tx: TransactionRequest,
        nonce: u64,
    ) -> Result<(), Error> {
        let mut guard = self.join_handle.lock().await;
        if let Some(join_handle) = guard.as_ref() {
            if !join_handle.is_finished() {
                return Err(Error::msg(
                    "Cannot monitor new transaction, previous transaction is in progress",
                ));
            }
        }

        let monitor_thread = TransactionMonitorThread::new(
            self.provider.clone(),
            self.config.clone(),
            nonce,
            self.error_notification_channel.clone(),
            self.metrics.clone(),
        );
        let join_handle = monitor_thread.spawn_monitoring_task(tx);
        *guard = Some(join_handle);
        Ok(())
    }

    pub async fn is_transaction_in_progress(&self) -> Result<bool, Error> {
        let guard = self.join_handle.lock().await;
        if let Some(join_handle) = guard.as_ref() {
            return Ok(!join_handle.is_finished());
        }
        Ok(false)
    }
}

impl TransactionMonitorThread {
    pub fn new(
        provider: Arc<WsProvider>,
        config: TransactionMonitorConfig,
        nonce: u64,
        error_notification_channel: Sender<TransactionError>,
        metrics: Arc<Metrics>,
    ) -> Self {
        Self {
            provider,
            config,
            nonce,
            error_notification_channel,
            metrics,
        }
    }
    pub fn spawn_monitoring_task(self, mut tx: TransactionRequest) -> JoinHandle<()> {
        tokio::spawn(async move {
            tx.set_nonce(self.nonce);
            if !matches!(tx.buildable_type(), Some(TxType::Eip1559 | TxType::Eip4844)) {
                self.send_error_signal(TransactionError::UnsupportedTransactionType)
                    .await;
                return;
            }

            debug!("Monitoring tx with nonce: {}  max_fee_per_gas: {:?}, max_priority_fee_per_gas: {:?}, max_fee_per_blob_gas: {:?}", self.nonce, tx.max_fee_per_gas, tx.max_priority_fee_per_gas, tx.max_fee_per_blob_gas);

            // Initial gas tuning
            let mut max_priority_fee_per_gas = tx.max_priority_fee_per_gas.unwrap();
            let mut max_fee_per_gas = tx.max_fee_per_gas.unwrap();
            let mut max_fee_per_blob_gas = tx.max_fee_per_blob_gas;

            // increase priority fee by percentage, rest double
            max_fee_per_gas *= 2;
            max_priority_fee_per_gas +=
                max_priority_fee_per_gas * self.config.tx_fees_increase_percentage / 100;
            if let Some(max_fee_per_blob_gas) = &mut max_fee_per_blob_gas {
                *max_fee_per_blob_gas *= 2;
            }

            if max_priority_fee_per_gas < self.config.min_priority_fee_per_gas_wei {
                let diff = self.config.min_priority_fee_per_gas_wei - max_priority_fee_per_gas;
                max_fee_per_gas += diff;
                max_priority_fee_per_gas += diff;
            }

            self.metrics.inc_batch_sent();
            // Sending attempts loop
            let mut tx_hashes = Vec::new();
            for sending_attempt in 0..self.config.max_attempts_to_send_tx {
                let mut tx_clone = tx.clone();
                self.set_tx_parameters(
                    &mut tx_clone,
                    max_fee_per_gas,
                    max_priority_fee_per_gas,
                    max_fee_per_blob_gas,
                );

                let l1_block_at_send = match self.provider.get_block_number().await {
                    Ok(block_number) => block_number,
                    Err(e) => {
                        error!("Failed to get L1 block number: {}", e);
                        self.send_error_signal(TransactionError::GetBlockNumberFailed)
                            .await;
                        return;
                    }
                };

                let pending_tx = match self.handle_transaction_send(tx_clone, &tx_hashes, sending_attempt as u64).await {
                    Some(pending_tx) => pending_tx,
                    None => return,
                };

                let tx_hash = *pending_tx.tx_hash();
                tx_hashes.push(tx_hash);

                debug!("{} tx nonce: {}, attempt: {}, l1_block: {}, hash: {},  max_fee_per_gas: {}, max_priority_fee_per_gas: {}, max_fee_per_blob_gas: {:?}",
                    if sending_attempt == 0 { "🟢 Send" } else { "🟡 Replace" },
                    self.nonce,
                    sending_attempt,
                    l1_block_at_send,
                    tx_hash,
                    max_fee_per_gas,
                    max_priority_fee_per_gas,
                    max_fee_per_blob_gas
                );

                if self
                    .is_transaction_handled_by_builder(
                        pending_tx.provider().clone(),
                        tx_hash,
                        l1_block_at_send,
                        sending_attempt as u64,
                    )
                    .await
                {
                    return;
                }

                // increase fees for next attempt
                // replacement requires 100% more for penalty
                max_fee_per_gas += max_fee_per_gas;
                max_priority_fee_per_gas += max_priority_fee_per_gas;
                if let Some(max_fee_per_blob_gas) = &mut max_fee_per_blob_gas {
                    *max_fee_per_blob_gas += *max_fee_per_blob_gas;
                }
            }

            error!(
                "Transaction {} with nonce {} not confirmed after {} attempts",
                if let Some(tx_hash) = tx_hashes.last() {
                    tx_hash.to_string()
                } else {
                    "unknown".to_string()
                },
                self.nonce,
                self.config.max_attempts_to_send_tx
            );

            self.send_error_signal(TransactionError::NotConfirmed).await;
        })
    }

    /// Returns true if transaction removed from mempool for any reason
    async fn is_transaction_handled_by_builder(
        &self,
        root_provider: RootProvider<alloy::network::Ethereum>,
        tx_hash: B256,
        l1_block_at_send: u64,
        sending_attempt: u64,
    ) -> bool {
        loop {
            let check_tx = PendingTransactionBuilder::new(root_provider.clone(), tx_hash);
            let tx_status = self.check_tx_receipt(check_tx, sending_attempt).await;
            match tx_status {
                TxStatus::Confirmed(_) => return true,
                TxStatus::Failed(_) => {
                    self.send_error_signal(TransactionError::TransactionReverted)
                        .await;
                    return true;
                }
                TxStatus::Pending => {} // Continue with retry attempts
            }
            // Check if L1 block number has changed since sending the tx
            // If not, check tx again and wait more
            let current_l1_height = match self.provider.get_block_number().await {
                Ok(block_number) => block_number,
                Err(e) => {
                    error!("Failed to get L1 block number: {}", e);
                    self.send_error_signal(TransactionError::GetBlockNumberFailed)
                        .await;
                    return true;
                }
            };
            if current_l1_height != l1_block_at_send {
                break;
            }
            debug!(
                "🟤 Missing block wait more for tx with nonce {}. Current L1 height: {}, L1 height at send: {}",
                self.nonce, current_l1_height, l1_block_at_send
            );
        }

        false
    }

    async fn handle_transaction_send(
        &self,
        tx: TransactionRequest,
        previous_tx_hashes: &Vec<B256>,
        sending_attempt: u64,
    ) -> Option<PendingTransactionBuilder<alloy::network::Ethereum>> {
        match self.provider.send_transaction(tx).await {
            Ok(tx) => Some(tx),
            Err(e) => {
                if let RpcError::ErrorResp(err) = &e {
                    if err.message.contains("nonce too low") {
                        let status = self.verify_tx_included(previous_tx_hashes, sending_attempt).await;
                        match status {
                            TxStatus::Confirmed(_) => return None,
                            _ => {
                                self.send_error_signal(TransactionError::TransactionReverted)
                                    .await;
                                return None;
                            }
                        }
                    }
                }
                // TODO if it is not revert then rebuild rpc client and retry on rpc error
                error!("Failed to send transaction: {}", e);
                self.send_error_signal(TransactionError::TransactionReverted)
                    .await;
                return None;
            }
        }
    }

    async fn send_error_signal(&self, error: TransactionError) {
        if let Err(e) = self.error_notification_channel.send(error).await {
            error!("Failed to send transaction error signal: {}", e);
        }
    }

    async fn verify_tx_included(&self, tx_hashes: &Vec<B256>, sending_attempt: u64) -> TxStatus {
        for tx_hash in tx_hashes {
            let tx = self.provider.get_transaction_by_hash(*tx_hash).await;
            if let Ok(Some(tx)) = tx {
                if let Some(block_number) = tx.block_number {
                    info!(
                        "✅ Transaction {} confirmed in block {} while trying to replace it",
                        tx_hash, block_number
                    );
                    self.metrics.observe_batch_propose_tries(sending_attempt - 1);
                    self.metrics.inc_batch_confirmed();
                    return TxStatus::Confirmed(block_number);
                }
            }
        }

        let warning = format!("Transaction not found, checked hashes: {:?}", tx_hashes);
        warn!("{}", warning);
        TxStatus::Failed(warning)
    }

    async fn check_tx_receipt<N: Network>(
        &self,
        pending_tx: PendingTransactionBuilder<N>,
        sending_attempt: u64,
    ) -> TxStatus {
        let tx_hash = *pending_tx.tx_hash();
        let receipt = pending_tx
            .with_timeout(Some(self.config.delay_between_tx_attempts))
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
                    self.metrics.observe_batch_propose_tries(sending_attempt);
                    self.metrics.inc_batch_confirmed();
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

    fn set_tx_parameters(
        &self,
        tx: &mut TransactionRequest,
        max_fee_per_gas: u128,
        max_priority_fee_per_gas: u128,
        max_fee_per_blob_gas: Option<u128>,
    ) {
        tx.set_max_priority_fee_per_gas(max_priority_fee_per_gas);
        tx.set_max_fee_per_gas(max_fee_per_gas);
        if let Some(max_fee_per_blob_gas) = max_fee_per_blob_gas {
            tx.set_max_fee_per_blob_gas(max_fee_per_blob_gas);
        }
        tx.set_nonce(self.nonce.into());

        debug!(
            "Tx params, max_fee_per_gas: {:?}, max_priority_fee_per_gas: {:?}, max_fee_per_blob_gas: {:?}, gas limit: {:?}, nonce: {:?}", tx.max_fee_per_gas, tx.max_priority_fee_per_gas, tx.max_fee_per_blob_gas, tx.gas, tx.nonce,
        );
    }
}
