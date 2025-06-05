#[derive(Debug)]
pub enum TransactionError {
    EstimationFailed,
    EstimationTooEarly,
    TransactionReverted,
    NotConfirmed,
    UnsupportedTransactionType,
    GetBlockNumberFailed,
    TimestampTooLarge,
    InsufficientFunds,
}

impl std::fmt::Display for TransactionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
