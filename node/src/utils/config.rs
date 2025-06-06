use alloy::primitives::U256;
use std::time::Duration;
use tracing::{info, warn};

use crate::taiko::blob::constants::MAX_BLOB_DATA_SIZE;

pub struct Config {
    pub taiko_geth_ws_rpc_url: String,
    pub taiko_geth_auth_rpc_url: String,
    pub taiko_driver_url: String,
    pub avs_node_ecdsa_private_key: String,
    pub mev_boost_url: String,
    pub l1_ws_rpc_url: String,
    pub l1_beacon_url: String,
    pub l1_slot_duration_sec: u64,
    pub l1_slots_per_epoch: u64,
    pub preconf_heartbeat_ms: u64,
    pub msg_expiry_sec: u64,
    pub contract_addresses: L1ContractAddresses,
    pub jwt_secret_file_path: String,
    pub rpc_l2_execution_layer_timeout: Duration,
    pub rpc_driver_preconf_timeout: Duration,
    pub rpc_driver_status_timeout: Duration,
    pub taiko_anchor_address: String,
    pub handover_window_slots: u64,
    pub handover_start_buffer_ms: u64,
    pub l1_height_lag: u64,
    pub max_bytes_size_of_batch: u64,
    pub max_blocks_per_batch: u16,
    pub max_time_shift_between_blocks_sec: u64,
    pub max_anchor_height_offset_reduction: u64,
    pub min_priority_fee_per_gas_wei: u64,
    pub tx_fees_increase_percentage: u64,
    pub max_attempts_to_send_tx: u64,
    pub max_attempts_to_wait_tx: u64,
    pub delay_between_tx_attempts_sec: u64,
    pub threshold_eth: U256,
    pub threshold_taiko: U256,
    pub simulate_not_submitting_at_the_end_of_epoch: bool,
    pub max_bytes_per_tx_list: u64,
    pub throttling_factor: u64,
    pub min_bytes_per_tx_list: u64,
}

#[derive(Debug)]
pub struct L1ContractAddresses {
    pub taiko_inbox: String,
    pub preconf_whitelist: String,
    pub preconf_router: String,
    pub taiko_wrapper: String,
    #[cfg(feature = "extra-gas-percentage")]
    pub extra_gas_percentage: u64,
}

impl Config {
    pub fn read_env_variables() -> Self {
        // Load environment variables from .env file
        dotenvy::dotenv().ok();

        let default_empty_address = "0x0000000000000000000000000000000000000000".to_string();

        const AVS_NODE_ECDSA_PRIVATE_KEY: &str = "AVS_NODE_ECDSA_PRIVATE_KEY";
        let avs_node_ecdsa_private_key =
            std::env::var(AVS_NODE_ECDSA_PRIVATE_KEY).unwrap_or_else(|_| {
                warn!(
                    "No AVS node ECDSA private key found in {} env var, using default",
                    AVS_NODE_ECDSA_PRIVATE_KEY
                );
                "0x4c0883a69102937d6231471b5dbb6204fe512961708279f2e3e8a5d4b8e3e3e8".to_string()
            });

        const TAIKO_INBOX_ADDRESS: &str = "TAIKO_INBOX_ADDRESS";
        let taiko_inbox = std::env::var(TAIKO_INBOX_ADDRESS).unwrap_or_else(|_| {
            warn!(
                "No TaikoL1 contract address found in {} env var, using default",
                TAIKO_INBOX_ADDRESS
            );
            default_empty_address.clone()
        });

        const PRECONF_WHITELIST_ADDRESS: &str = "PRECONF_WHITELIST_ADDRESS";
        let preconf_whitelist = std::env::var(PRECONF_WHITELIST_ADDRESS).unwrap_or_else(|_| {
            warn!(
                "No PreconfWhitelist contract address found in {} env var, using default",
                PRECONF_WHITELIST_ADDRESS
            );
            default_empty_address.clone()
        });

        const PRECONF_ROUTER_ADDRESS: &str = "PRECONF_ROUTER_ADDRESS";
        let preconf_router = std::env::var(PRECONF_ROUTER_ADDRESS).unwrap_or_else(|_| {
            warn!(
                "No PreconfRouter contract address found in {} env var, using default",
                PRECONF_ROUTER_ADDRESS
            );
            default_empty_address.clone()
        });

        const TAIKO_WRAPPER_ADDRESS: &str = "TAIKO_WRAPPER_ADDRESS";
        let taiko_wrapper = std::env::var(TAIKO_WRAPPER_ADDRESS).unwrap_or_else(|_| {
            warn!(
                "No TaikoWrapper contract address found in {} env var, using default",
                TAIKO_WRAPPER_ADDRESS
            );
            default_empty_address.clone()
        });

        #[cfg(feature = "extra-gas-percentage")]
        let extra_gas_percentage = std::env::var("EXTRA_GAS_PERCENTAGE")
            .unwrap_or("5".to_string())
            .parse::<u64>()
            .expect("EXTRA_GAS_PERCENTAGE must be a number");

        let contract_addresses = L1ContractAddresses {
            taiko_inbox,
            preconf_whitelist,
            preconf_router,
            taiko_wrapper,
            #[cfg(feature = "extra-gas-percentage")]
            extra_gas_percentage,
        };

        let l1_slot_duration_sec = std::env::var("L1_SLOT_DURATION_SEC")
            .unwrap_or("12".to_string())
            .parse::<u64>()
            .inspect(|&val| {
                if val == 0 {
                    panic!("L1_SLOT_DURATION_SEC must be a positive number");
                }
            })
            .expect("L1_SLOT_DURATION_SEC must be a number");

        let l1_slots_per_epoch = std::env::var("L1_SLOTS_PER_EPOCH")
            .unwrap_or("32".to_string())
            .parse::<u64>()
            .inspect(|&val| {
                if val == 0 {
                    panic!("L1_SLOTS_PER_EPOCH must be a positive number");
                }
            })
            .expect("L1_SLOTS_PER_EPOCH must be a number");

        let preconf_heartbeat_ms = std::env::var("PRECONF_HEARTBEAT_MS")
            .unwrap_or("2000".to_string())
            .parse::<u64>()
            .inspect(|&val| {
                if val == 0 {
                    panic!("PRECONF_HEARTBEAT_MS must be a positive number");
                }
            })
            .expect("PRECONF_HEARTBEAT_MS must be a number");

        let msg_expiry_sec = std::env::var("MSG_EXPIRY_SEC")
            .unwrap_or("3600".to_string())
            .parse::<u64>()
            .expect("MSG_EXPIRY_SEC must be a number");

        let jwt_secret_file_path = std::env::var("JWT_SECRET_FILE_PATH").unwrap_or_else(|_| {
            warn!(
                "No JWT secret file path found in {} env var, using default",
                "JWT_SECRET_FILE_PATH"
            );
            "/tmp/jwtsecret".to_string()
        });

        let rpc_driver_preconf_timeout = std::env::var("RPC_DRIVER_PRECONF_TIMEOUT_MS")
            .unwrap_or("60000".to_string())
            .parse::<u64>()
            .expect("RPC_DRIVER_PRECONF_TIMEOUT_MS must be a number");
        let rpc_driver_preconf_timeout = Duration::from_millis(rpc_driver_preconf_timeout);

        let rpc_driver_status_timeout = std::env::var("RPC_DRIVER_STATUS_TIMEOUT_MS")
            .unwrap_or("1000".to_string())
            .parse::<u64>()
            .expect("RPC_DRIVER_STATUS_TIMEOUT_MS must be a number");
        let rpc_driver_status_timeout = Duration::from_millis(rpc_driver_status_timeout);

        let rpc_l2_execution_layer_timeout = std::env::var("RPC_L2_EXECUTION_LAYER_TIMEOUT_MS")
            .unwrap_or("1000".to_string())
            .parse::<u64>()
            .expect("RPC_L2_EXECUTION_LAYER_TIMEOUT_MS must be a number");
        let rpc_l2_execution_layer_timeout = Duration::from_millis(rpc_l2_execution_layer_timeout);

        let taiko_anchor_address = std::env::var("TAIKO_ANCHOR_ADDRESS")
            .unwrap_or("0x1670010000000000000000000000000000010001".to_string());

        let handover_window_slots = std::env::var("HANDOVER_WINDOW_SLOTS")
            .unwrap_or("4".to_string())
            .parse::<u64>()
            .expect("HANDOVER_WINDOW_SLOTS must be a number");

        let handover_start_buffer_ms = std::env::var("HANDOVER_START_BUFFER_MS")
            .unwrap_or("6000".to_string())
            .parse::<u64>()
            .expect("HANDOVER_START_BUFFER_MS must be a number");

        let l1_height_lag = std::env::var("L1_HEIGHT_LAG")
            .unwrap_or("4".to_string())
            .parse::<u64>()
            .expect("L1_HEIGHT_LAG must be a number");

        let blobs_per_batch = std::env::var("BLOBS_PER_BATCH")
            .unwrap_or("3".to_string())
            .parse::<u64>()
            .expect("BLOBS_PER_BATCH must be a number");

        let max_bytes_size_of_batch = u64::try_from(MAX_BLOB_DATA_SIZE)
            .expect("MAX_BLOB_DATA_SIZE must be a u64 number")
            * blobs_per_batch;

        let max_blocks_per_batch = std::env::var("MAX_BLOCKS_PER_BATCH")
            .unwrap_or("0".to_string())
            .parse::<u16>()
            .expect("MAX_BLOCKS_PER_BATCH must be a number");

        let max_time_shift_between_blocks_sec = std::env::var("MAX_TIME_SHIFT_BETWEEN_BLOCKS_SEC")
            .unwrap_or("255".to_string())
            .parse::<u64>()
            .expect("MAX_TIME_SHIFT_BETWEEN_BLOCKS_SEC must be a number");

        // It is the slot window in which we want to call the proposeBatch transaction
        // and avoid exceeding the MAX_ANCHOR_HEIGHT_OFFSET.
        let max_anchor_height_offset_reduction =
            std::env::var("MAX_ANCHOR_HEIGHT_OFFSET_REDUCTION_VALUE")
                .unwrap_or("10".to_string())
                .parse::<u64>()
                .expect("MAX_ANCHOR_HEIGHT_OFFSET_REDUCTION_VALUE must be a number");
        if max_anchor_height_offset_reduction < 5 {
            warn!(
                "MAX_ANCHOR_HEIGHT_OFFSET_REDUCTION_VALUE is less than 5: you have a small number of slots to call the proposeBatch transaction"
            );
        }

        let min_priority_fee_per_gas_wei = std::env::var("MIN_PRIORITY_FEE_PER_GAS_WEI")
            .unwrap_or("10000000000".to_string()) // 10 Gwei
            .parse::<u64>()
            .expect("MIN_PRIORITY_FEE_PER_GAS_WEI must be a number");

        let tx_fees_increase_percentage = std::env::var("TX_FEES_INCREASE_PERCENTAGE")
            .unwrap_or("20".to_string())
            .parse::<u64>()
            .expect("TX_FEES_INCREASE_PERCENTAGE must be a number");

        let max_attempts_to_send_tx = std::env::var("MAX_ATTEMPTS_TO_SEND_TX")
            .unwrap_or("4".to_string())
            .parse::<u64>()
            .expect("MAX_ATTEMPTS_TO_SEND_TX must be a number");

        let max_attempts_to_wait_tx = std::env::var("MAX_ATTEMPTS_TO_WAIT_TX")
            .unwrap_or("5".to_string())
            .parse::<u64>()
            .expect("MAX_ATTEMPTS_TO_WAIT_TX must be a number");

        let delay_between_tx_attempts_sec = std::env::var("DELAY_BETWEEN_TX_ATTEMPTS_SEC")
            .unwrap_or("15".to_string())
            .parse::<u64>()
            .expect("DELAY_BETWEEN_TX_ATTEMPTS_SEC must be a number");

        // 0.5 ETH
        let threshold_eth =
            std::env::var("THRESHOLD_ETH").unwrap_or("500000000000000000".to_string());
        let threshold_eth =
            U256::from_str_radix(&threshold_eth, 10).expect("THRESHOLD_ETH must be a number");

        // 1000 TAIKO
        let threshold_taiko =
            std::env::var("THRESHOLD_TAIKO").unwrap_or("10000000000000000000000".to_string());
        let threshold_taiko =
            U256::from_str_radix(&threshold_taiko, 10).expect("THRESHOLD_TAIKO must be a number");

        let simulate_not_submitting_at_the_end_of_epoch =
            std::env::var("SIMULATE_NOT_SUBMITTING_AT_THE_END_OF_EPOCH")
                .unwrap_or("false".to_string())
                .parse::<bool>()
                .expect("SIMULATE_NOT_SUBMITTING_AT_THE_END_OF_EPOCH must be a boolean");

        let max_bytes_per_tx_list = std::env::var("MAX_BYTES_PER_TX_LIST")
            .unwrap_or(MAX_BLOB_DATA_SIZE.to_string())
            .parse::<u64>()
            .expect("MAX_BYTES_PER_TX_LIST must be a number");

        // The throttling factor is used to reduce the max bytes per tx list exponentially.
        let throttling_factor = std::env::var("THROTTLING_FACTOR")
            .unwrap_or("2".to_string())
            .parse::<u64>()
            .expect("THROTTLING_FACTOR must be a number");

        let min_bytes_per_tx_list = std::env::var("MIN_BYTES_PER_TX_LIST")
            .unwrap_or("8192".to_string()) // 8KB
            .parse::<u64>()
            .expect("MIN_BYTES_PER_TX_LIST must be a number");

        let config = Self {
            taiko_geth_ws_rpc_url: std::env::var("TAIKO_GETH_WS_RPC_URL")
                .unwrap_or("ws://127.0.0.1:1234".to_string()),
            taiko_geth_auth_rpc_url: std::env::var("TAIKO_GETH_AUTH_RPC_URL")
                .unwrap_or("http://127.0.0.1:1235".to_string()),
            taiko_driver_url: std::env::var("TAIKO_DRIVER_URL")
                .unwrap_or("http://127.0.0.1:1236".to_string()),

            avs_node_ecdsa_private_key,
            mev_boost_url: std::env::var("MEV_BOOST_URL")
                .unwrap_or("http://127.0.0.1:8080".to_string()),
            l1_ws_rpc_url: std::env::var("L1_WS_RPC_URL").unwrap_or("wss://127.0.0.1".to_string()),
            l1_beacon_url: std::env::var("L1_BEACON_URL")
                .unwrap_or("http://127.0.0.1:4000".to_string()),
            l1_slot_duration_sec,
            l1_slots_per_epoch,
            preconf_heartbeat_ms,
            msg_expiry_sec,
            contract_addresses,
            jwt_secret_file_path,
            rpc_l2_execution_layer_timeout,
            rpc_driver_preconf_timeout,
            rpc_driver_status_timeout,
            taiko_anchor_address,
            handover_window_slots,
            handover_start_buffer_ms,
            l1_height_lag,
            max_bytes_size_of_batch,
            max_blocks_per_batch,
            max_time_shift_between_blocks_sec,
            max_anchor_height_offset_reduction,
            min_priority_fee_per_gas_wei,
            tx_fees_increase_percentage,
            max_attempts_to_send_tx,
            max_attempts_to_wait_tx,
            delay_between_tx_attempts_sec,
            threshold_eth,
            threshold_taiko,
            simulate_not_submitting_at_the_end_of_epoch,
            max_bytes_per_tx_list,
            throttling_factor,
            min_bytes_per_tx_list,
        };

        info!(
            r#"
Configuration:
Taiko geth WS RPC URL: {},
Taiko geth auth RPC URL: {},
Taiko driver URL: {},
MEV Boost URL: {},
L1 WS URL: {},
Consensus layer URL: {}
L1 slot duration: {}s
L1 slots per epoch: {}
L2 slot duration (heart beat): {}
Preconf registry expiry: {}s
Contract addresses: {:#?}
jwt secret file path: {}
rpc L2 EL timeout: {}ms
rpc driver preconf timeout: {}ms
rpc driver status timeout: {}ms
taiko anchor address: {}
handover window slots: {}
handover start buffer: {}ms
l1 height lag: {}
max bytes per tx list from taiko driver: {}
throttling factor: {}
min pending tx list size: {} bytes
max bytes size of batch: {}
max blocks per batch value: {}
max time shift between blocks: {}s
max anchor height offset reduction value: {}
min priority fee per gas wei: {}
tx fees increase percentage: {}
max attempts to send tx: {}
max attempts to wait tx: {}
delay between tx attempts: {}s
threshold_eth: {}
threshold_taiko: {}
simulate not submitting at the end of epoch: {}
"#,
            config.taiko_geth_ws_rpc_url,
            config.taiko_geth_auth_rpc_url,
            config.taiko_driver_url,
            config.mev_boost_url,
            config.l1_ws_rpc_url,
            config.l1_beacon_url,
            config.l1_slot_duration_sec,
            config.l1_slots_per_epoch,
            config.preconf_heartbeat_ms,
            config.msg_expiry_sec,
            config.contract_addresses,
            config.jwt_secret_file_path,
            config.rpc_l2_execution_layer_timeout.as_millis(),
            config.rpc_driver_preconf_timeout.as_millis(),
            config.rpc_driver_status_timeout.as_millis(),
            config.taiko_anchor_address,
            config.handover_window_slots,
            config.handover_start_buffer_ms,
            config.l1_height_lag,
            config.max_bytes_per_tx_list,
            config.throttling_factor,
            config.min_bytes_per_tx_list,
            config.max_bytes_size_of_batch,
            config.max_blocks_per_batch,
            config.max_time_shift_between_blocks_sec,
            config.max_anchor_height_offset_reduction,
            config.min_priority_fee_per_gas_wei,
            config.tx_fees_increase_percentage,
            config.max_attempts_to_send_tx,
            config.max_attempts_to_wait_tx,
            config.delay_between_tx_attempts_sec,
            threshold_eth,
            threshold_taiko,
            config.simulate_not_submitting_at_the_end_of_epoch,
        );

        config
    }
}
