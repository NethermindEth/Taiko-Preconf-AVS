use crate::utils::config::L1ContractAddresses;

pub struct EthereumL1Config {
    pub execution_ws_rpc_url: String,
    pub avs_node_ecdsa_private_key: String,
    pub contract_addresses: L1ContractAddresses,
    pub consensus_rpc_url: String,
    pub min_priority_fee_per_gas_wei: u64,
    pub tx_fees_increase_percentage: u64,
    pub slot_duration_sec: u64,
    pub slots_per_epoch: u64,
    pub preconf_heartbeat_ms: u64,
    pub max_attempts_to_send_tx: u64,
    pub delay_between_tx_attempts_sec: u64,
}
