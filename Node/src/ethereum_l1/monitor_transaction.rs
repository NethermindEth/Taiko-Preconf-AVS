use crate::ethereum_l1::ws_provider::WsProvider;
use alloy::{
    consensus::{TxEip4844Variant, TxEnvelope},
    network::{TransactionBuilder, TransactionBuilder4844},
    primitives::{Address, TxKind, B256},
    providers::{ext::DebugApi, Provider},
    rpc::types::{trace::geth::GethDebugTracingOptions, Transaction, TransactionRequest},
};
use std::{sync::Arc, time::Duration};
use tokio::{task::JoinHandle, time};
use tracing::{error, info, trace, warn};

// Transaction status enum
#[derive(Debug, Clone, PartialEq)]
pub enum TxStatus {
    Confirmed(u64), // Block number
    Failed(String), // Error message
}

/// Monitor a transaction until it is confirmed or fails.
/// Spawns a new tokio task to monitor the transaction.
pub async fn monitor_transaction(provider: Arc<WsProvider>, tx_hash: B256) -> JoinHandle<TxStatus> {
    tokio::spawn(async move {
        let max_attempts = 50; //TODO move to config
        let delay = Duration::from_secs(2);

        for attempt in 0..max_attempts {
            match provider.get_transaction_receipt(tx_hash).await {
                Ok(Some(receipt)) => {
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
                    } else {
                        if let Some(block_number) = receipt.block_number {
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
                }
                Ok(None) => {
                    trace!(
                        "Transaction {} still pending (attempt {}/{})",
                        tx_hash,
                        attempt + 1,
                        max_attempts
                    );
                    time::sleep(delay).await;
                }
                Err(e) => {
                    error!("Error checking transaction {}: {}", tx_hash, e);
                    time::sleep(delay).await;
                }
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
    let revert_reason = match provider
        .call(&call_request)
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
    return error_msg;
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
            error_message.push_str(&error_content);
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
    match tx_details.inner {
        TxEnvelope::Eip1559(tx) => {
            let to = match tx.tx().to {
                TxKind::Call(to) => to,
                _ => Address::default(),
            };
            TransactionRequest::default()
                .with_from(tx_details.from)
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
                .with_from(tx_details.from)
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
                .with_from(tx_details.from)
                .with_to(to)
                .with_input(tx.tx().input.clone())
                .with_value(tx.tx().value)
                .with_gas_limit(tx.tx().gas_limit)
        }
        TxEnvelope::Eip4844(tx) => {
            let tx = tx.tx();
            match tx {
                TxEip4844Variant::TxEip4844(tx) => TransactionRequest::default()
                    .with_from(tx_details.from)
                    .with_to(tx.to)
                    .with_input(tx.input.clone())
                    .with_value(tx.value)
                    .with_gas_limit(tx.gas_limit),
                TxEip4844Variant::TxEip4844WithSidecar(tx) => TransactionRequest::default()
                    .with_from(tx_details.from)
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
