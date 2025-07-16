use super::web3signer::Web3Signer;
use alloy::providers::{
    Identity, RootProvider,
    fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
};
use std::sync::Arc;

pub type WsProvider = FillProvider<
    JoinFill<
        Identity,
        JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
    >,
    RootProvider,
>;

pub enum Signer {
    Web3signer(Arc<Web3Signer>),
    PrivateKey(String),
}
