#[derive(Debug)]
pub enum TransactionError {
    EstimationFailed,
    EstimationTooEarly,
    TransactionReverted,
    Web3SignerFailed,
    NotConfirmed,
    UnsupportedTransactionType,
    GetBlockNumberFailed,
    TimestampTooLarge,
    InsufficientFunds,
    ReanchorRequired,
    BuildTransactionFailed,
    OldestForcedInclusionDue,
}

impl std::fmt::Display for TransactionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
