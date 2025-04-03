use crate::ethereum_l1::ws_provider::WsProvider;
use alloy::{
    consensus::{Transaction as ConsensusTransaction, TxEip4844Variant, TxEnvelope},
    eips::{eip2718::Encodable2718, BlockNumberOrTag},
    network::{Ethereum, TransactionBuilder, TransactionBuilder4844},
    primitives::{Address, FixedBytes, TxKind, B256},
    providers::{
        ext::DebugApi, PendingTransactionBuilder, PendingTransactionError, Provider,
        ProviderBuilder, WalletProvider, WatchTxError, WsConnect,
    },
    rpc::types::{trace::geth::GethDebugTracingOptions, Transaction, TransactionRequest},
};
use std::{sync::Arc, time::Duration};
use tokio::{task::JoinHandle, time};
use tracing::{debug, error, info, trace, warn};

// Transaction status enum
#[derive(Debug, Clone, PartialEq)]
pub enum TxStatus {
    Confirmed(u64), // Block number
    Failed(String), // Error message
}

/// Monitor a transaction until it is confirmed or fails.
/// Spawns a new tokio task to monitor the transaction.
pub fn monitor_transaction(
    provider: Arc<WsProvider>,
    tx: TransactionRequest,
    nonce: u64,
) -> JoinHandle<TxStatus> {
    tokio::spawn(async move {
        let max_attempts: u64 = 3; //TODO move to config
        let delay = Duration::from_millis(1000); //Duration::from_secs(12);
        let mut tx_hash = B256::ZERO;

        // const increase_percentage: u128 = 20;
        let mut max_priority_fee_per_gas = if tx.max_priority_fee_per_gas.is_none() {
            1_000_000_000
        } else {
            tx.max_priority_fee_per_gas.unwrap()
        };
        let mut max_fee_per_gas = if tx.max_fee_per_gas.is_none() {
            1_000_000_000
        } else {
            tx.max_fee_per_gas.unwrap()
        };
        let mut max_fee_per_blob_gas = if tx.max_fee_per_blob_gas.is_none() {
            1
        } else {
            tx.max_fee_per_blob_gas.unwrap()
        };

        for attempt in 0..max_attempts {
            let mut tx_clone = tx.clone();
            let pending_tx = if attempt > 0 {
                let block = provider
                    .get_block_by_number(BlockNumberOrTag::Latest)
                    .await
                    .unwrap()
                    .unwrap();
                let base_fee = block.header.base_fee_per_gas.unwrap() as u128;
                debug!("Base fee: {}", base_fee);

                if attempt == 1 {
                    max_fee_per_gas = base_fee * 2 + max_priority_fee_per_gas + attempt as u128 + 1;
                    max_priority_fee_per_gas += 10_000_000_000; // max_priority_fee_per_gas * increase_percentage / 100;
                } else {
                    max_fee_per_gas = max_fee_per_gas * 2 + 1; // second replacement requires 100% more for penalty
                    max_priority_fee_per_gas = max_priority_fee_per_gas * 2 + 1;
                }

                max_fee_per_blob_gas += max_fee_per_blob_gas + 1;

                tx_clone.set_max_priority_fee_per_gas(max_priority_fee_per_gas);
                tx_clone.set_max_fee_per_gas(max_fee_per_gas);
                tx_clone.set_max_fee_per_blob_gas(max_fee_per_blob_gas);

                tx_clone.set_nonce(nonce.into());

                debug!("Transaction type: {:?}", tx_clone.preferred_type());

                debug!("Sending transaction max_fee_per_gas: {}, max_priority_fee_per_gas: {}, max_fee_per_blob_gas: {} gas limit: {}, nonce: {}", tx_clone.max_fee_per_gas.unwrap(), tx_clone.max_priority_fee_per_gas.unwrap(), tx_clone.max_fee_per_blob_gas.unwrap(), tx_clone.gas.unwrap(), tx_clone.nonce.unwrap());

                match provider.send_transaction(tx_clone).await {
                    Ok(tx) => tx,
                    Err(e) => {
                        error!("Attempt {attempt}. Failed to send transaction: {:?}", e);
                        return TxStatus::Failed(e.to_string());
                    }
                }
            } else {
                debug!("Sending transaction max_fee_per_gas: {}, max_priority_fee_per_gas: {}, max_fee_per_blob_gas: {} gas limit: {}, nonce: {:?}", tx_clone.max_fee_per_gas.unwrap(), tx_clone.max_priority_fee_per_gas.unwrap(), tx_clone.max_fee_per_blob_gas.unwrap(), tx_clone.gas.unwrap(), tx_clone.nonce);

                match provider.send_transaction(tx_clone).await {
                    Ok(tx) => tx,
                    Err(e) => {
                        error!("Failed to send transaction: {:?}", e);
                        return TxStatus::Failed(e.to_string());
                    }
                }
            };

            tx_hash = *pending_tx.tx_hash();
            debug!("Transaction hash: {}", tx_hash);
            let receipt = pending_tx.with_timeout(Some(delay)).get_receipt().await;

            match receipt {
                Ok(receipt) => {
                    if receipt.status() {
                        let block_number = if let Some(block_number) = receipt.block_number {
                            block_number
                        } else {
                            warn!("Block number not found for transaction {}", tx_hash);
                            0
                        };

                        info!(
                            "Transaction {} confirmed in block {}",
                            tx_hash, block_number
                        );
                        return TxStatus::Confirmed(block_number);
                    } else if let Some(block_number) = receipt.block_number {
                        return TxStatus::Failed(
                            check_for_revert_reason(tx_hash, &provider, block_number).await,
                        );
                    } else {
                        let error_msg =
                            format!("Transaction {tx_hash} failed, but block number not found");
                        error!("{}", error_msg);
                        return TxStatus::Failed(error_msg);
                    }
                }
                Err(e) => match e {
                    PendingTransactionError::TxWatcher(WatchTxError::Timeout) => {
                        debug!("Transaction watcher timeout");
                    }
                    _ => {
                        error!("Error checking transaction {}: {}", tx_hash, e);
                    }
                },
            }
        }

        let error_msg = format!(
            "Transaction {} not confirmed after {} attempts",
            tx_hash, max_attempts
        );
        error!("{}", error_msg);
        TxStatus::Failed(error_msg)
    })
}

async fn check_for_revert_reason(
    tx_hash: B256,
    provider: &WsProvider,
    block_number: u64,
) -> String {
    let default_options = GethDebugTracingOptions::default();
    let trace = provider
        .debug_trace_transaction(tx_hash, default_options)
        .await;

    let trace_errors = if let Ok(trace) = trace {
        find_errors_from_trace(&format!("{:?}", trace))
    } else {
        None
    };

    let tx_details = match provider.get_transaction_by_hash(tx_hash).await {
        Ok(Some(tx)) => tx,
        _ => {
            let error_msg = format!("Transaction {} failed", tx_hash);
            error!("{}", error_msg);
            return error_msg;
        }
    };

    let call_request = get_tx_request_for_call(tx_details);
    let revert_reason = match provider.call(call_request).block(block_number.into()).await {
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
