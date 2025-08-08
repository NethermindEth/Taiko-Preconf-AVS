use super::web3signer::Web3Signer;
use std::sync::Arc;

pub enum Signer {
    Web3signer(Arc<Web3Signer>),
    PrivateKey(String),
}
