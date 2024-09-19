use alloy::{network::{Ethereum, EthereumWallet}, pubsub::PubSubFrontend};

pub type ECDSASignature = [u8; 65]; // ECDSA 65 bytes signature
pub type BLSCompressedPublicKey = [u8; 48];

pub type PreconferAddress = [u8; 20];
pub const PRECONFER_ADDRESS_ZERO: PreconferAddress = [0u8; 20];

pub type L2TxListHash = [u8; 32];

pub type Slot = u64;
pub type Epoch = u64;

pub type WsProvider = alloy::providers::fillers::FillProvider<alloy::providers::fillers::JoinFill<alloy::providers::fillers::JoinFill<alloy::providers::fillers::JoinFill<alloy::providers::fillers::JoinFill<alloy::providers::Identity, alloy::providers::fillers::GasFiller>, alloy::providers::fillers::NonceFiller>, alloy::providers::fillers::ChainIdFiller>, alloy::providers::fillers::WalletFiller<EthereumWallet>>, alloy::providers::RootProvider<PubSubFrontend>, PubSubFrontend, Ethereum>;

// TODO for future usage
// pub type BLSUncompressedPublicKey = [u8; 96];
// pub type BLSSignature = [u8; 96];
