#[derive(Debug)]
pub enum TransactionError {
    TransactionReverted,
    UnsupportedTransactionType,
    GetBlockNumberFailed,
}
