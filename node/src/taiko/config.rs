use crate::utils::types::PreconferAddress;
use alloy::{
    primitives::{Address, B256},
    providers::{
        Identity, RootProvider,
        fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
    },
};
use std::{fmt, time::Duration};

pub const GOLDEN_TOUCH_PRIVATE_KEY: B256 = B256::new([
    0x92, 0x95, 0x43, 0x68, 0xaf, 0xd3, 0xca, 0xa1, 0xf3, 0xce, 0x3e, 0xad, 0x00, 0x69, 0xc1, 0xaf,
    0x41, 0x40, 0x54, 0xae, 0xfe, 0x1e, 0xf9, 0xae, 0xac, 0xc1, 0xbf, 0x42, 0x62, 0x22, 0xce, 0x38,
]);

pub const GOLDEN_TOUCH_ADDRESS: Address = Address::new([
    0x00, 0x00, 0x77, 0x77, 0x35, 0x36, 0x7b, 0x36, 0xbc, 0x9b, 0x61, 0xc5, 0x00, 0x22, 0xd9, 0xd0,
    0x70, 0x0d, 0xb4, 0xec,
]);

pub type WsProvider = FillProvider<
    JoinFill<
        Identity,
        JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
    >,
    RootProvider,
>;

pub enum OperationType {
    Preconfirm,
    Reanchor,
    Status,
}

impl fmt::Display for OperationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            OperationType::Preconfirm => "Preconfirm",
            OperationType::Reanchor => "Reanchor",
            OperationType::Status => "Status",
        };
        write!(f, "{}", s)
    }
}

#[derive(Clone)]
pub struct TaikoConfig {
    pub taiko_geth_ws_url: String,
    pub taiko_geth_auth_url: String,
    pub driver_url: String,
    pub jwt_secret_bytes: [u8; 32],
    pub preconfer_address: PreconferAddress,
    pub taiko_anchor_address: String,
    pub max_bytes_per_tx_list: u64,
    pub min_bytes_per_tx_list: u64,
    pub throttling_factor: u64,
    pub rpc_geth_timeout: Duration,
    pub rpc_driver_preconf_timeout: Duration,
    pub rpc_driver_status_timeout: Duration,
}
