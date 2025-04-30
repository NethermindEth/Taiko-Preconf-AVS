#[derive(Debug)]
pub enum TransactionError {
    TransactionReverted,
    NotConfirmed,
    UnsupportedTransactionType,
    GetBlockNumberFailed,
}
