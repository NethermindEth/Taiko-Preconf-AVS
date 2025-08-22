use alloy::providers::DynProvider;

/// Execution layer extension trait.
/// Enables additional features to the execution layer, specific for URC or whitelist implementation.
pub trait ELExtension: Send + Sync {
    fn new(provider: DynProvider) -> Self;
}
