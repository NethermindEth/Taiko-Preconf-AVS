#[derive(Debug)]
pub enum TransactionResult {
    Success,
    EstimationFailed,
    EstimationTooEarly,
    TransactionReverted,
    NotConfirmed,
    UnsupportedTransactionType,
    GetBlockNumberFailed,
    TimestampTooLarge,
    TransactionInProgress,
}

impl std::fmt::Display for TransactionResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
