#[derive(Debug)]
pub enum TransactionError {
    EstimationFailed,
    TransactionReverted,
    NotConfirmed,
    UnsupportedTransactionType,
    GetBlockNumberFailed,
}

impl std::fmt::Display for TransactionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
