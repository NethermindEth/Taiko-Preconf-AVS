use super::ws_provider::WsProvider;
use alloy::{
    primitives::B256,
    providers::{Provider, ext::DebugApi},
    rpc::types::{Transaction, TransactionRequest, trace::geth::GethDebugTracingOptions},
};

pub async fn check_for_revert_reason(
    provider: &WsProvider,
    tx_hash: B256,
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
            return format!("Transaction {} failed", tx_hash);
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
    error_msg
}

fn get_tx_request_for_call(tx_details: Transaction) -> TransactionRequest {
    TransactionRequest::from_transaction(tx_details)
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
