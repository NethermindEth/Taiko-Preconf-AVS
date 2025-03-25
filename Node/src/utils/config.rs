use std::time::Duration;
use tracing::{info, warn};

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
    pub validator_index: u64,
    pub enable_preconfirmation: bool,
    pub jwt_secret_file_path: String,
    pub rpc_client_timeout: Duration,
    pub taiko_anchor_address: String,
    pub handover_window_slots: u64,
    pub handover_start_buffer_ms: u64,
    pub l1_height_lag: u64,
    pub max_bytes_size_of_batch: u64,
    pub max_blocks_per_batch: u64,
    pub max_time_shift_between_blocks_sec: u64,
    pub max_anchor_height_offset: u64,
}

#[derive(Debug)]
pub struct L1ContractAddresses {
    pub taiko_inbox: String,
    pub preconf_whitelist: String,
    pub preconf_router: String,
    #[cfg(feature = "extra_gas_percentage")]
    pub extra_gas_percentage: u64,
}

impl Config {
    pub fn read_env_variables() -> Self {
        // Load environment variables from .env file
        dotenv::dotenv().ok();

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

        #[cfg(feature = "extra_gas_percentage")]
        let extra_gas_percentage = std::env::var("EXTRA_GAS_PERCENTAGE")
            .unwrap_or("5".to_string())
            .parse::<u64>()
            .expect("EXTRA_GAS_PERCENTAGE must be a number");

        let contract_addresses = L1ContractAddresses {
            taiko_inbox,
            preconf_whitelist,
            preconf_router,
            #[cfg(feature = "extra_gas_percentage")]
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

        let validator_index = std::env::var("VALIDATOR_INDEX")
            .expect("VALIDATOR_INDEX env variable must be set")
            .parse::<u64>()
            .expect("VALIDATOR_INDEX must be a number");

        let enable_preconfirmation = std::env::var("ENABLE_PRECONFIRMATION")
            .unwrap_or("true".to_string())
            .parse::<bool>()
            .expect("ENABLE_PRECONFIRMATION must be a boolean");

        let jwt_secret_file_path = std::env::var("JWT_SECRET_FILE_PATH").unwrap_or_else(|_| {
            warn!(
                "No JWT secret file path found in {} env var, using default",
                "JWT_SECRET_FILE_PATH"
            );
            "/tmp/jwtsecret".to_string()
        });

        let rpc_client_timeout = std::env::var("RPC_CLIENT_TIMEOUT_SEC")
            .unwrap_or("10".to_string())
            .parse::<u64>()
            .expect("RPC_CLIENT_TIMEOUT_SEC must be a number");
        let rpc_client_timeout = Duration::from_secs(rpc_client_timeout);

        let taiko_anchor_address = std::env::var("TAIKO_ANCHOR_ADDRESS")
            .unwrap_or("0x1670010000000000000000000000000000010001".to_string());

        let handover_window_slots = std::env::var("HANDOVER_WINDOW_SLOTS")
            .unwrap_or("3".to_string())
            .parse::<u64>()
            .expect("HANDOVER_WINDOW_SLOTS must be a number");

        let handover_start_buffer_ms = std::env::var("HANDOVER_START_BUFFER_MS")
            .unwrap_or("500".to_string())
            .parse::<u64>()
            .expect("HANDOVER_START_BUFFER_MS must be a number");

        let l1_height_lag = std::env::var("L1_HEIGHT_LAG")
            .unwrap_or("5".to_string())
            .parse::<u64>()
            .expect("L1_HEIGHT_LAG must be a number");

        let max_bytes_size_of_batch = std::env::var("MAX_BYTES_SIZE_OF_BATCH")
            .unwrap_or("130044".to_string())
            .parse::<u64>()
            .expect("MAX_BYTES_SIZE_OF_BATCH must be a number");

        let max_blocks_per_batch = std::env::var("MAX_BLOCKS_PER_BATCH")
            .unwrap_or("4".to_string())
            .parse::<u64>()
            .expect("MAX_BLOCKS_PER_BATCH must be a number");

        let max_time_shift_between_blocks_sec = std::env::var("MAX_TIME_SHIFT_BETWEEN_BLOCKS_SEC")
            .unwrap_or("255".to_string())
            .parse::<u64>()
            .expect("MAX_TIME_SHIFT_BETWEEN_BLOCKS_SEC must be a number");

        let max_anchor_height_offset = std::env::var("MAX_ANCHOR_HEIGHT_OFFSET")
            .unwrap_or("54".to_string())
            .parse::<u64>()
            .expect("MAX_ANCHOR_HEIGHT_OFFSET must be a number");

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
            validator_index,
            enable_preconfirmation,
            jwt_secret_file_path,
            rpc_client_timeout,
            taiko_anchor_address,
            handover_window_slots,
            handover_start_buffer_ms,
            l1_height_lag,
            max_bytes_size_of_batch,
            max_blocks_per_batch,
            max_time_shift_between_blocks_sec,
            max_anchor_height_offset,
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
L1 slot duration: {}
L1 slots per epoch: {}
L2 slot duration (heart beat): {}
Preconf registry expiry seconds: {}
Contract addresses: {:#?}
validator index: {}
enable preconfirmation: {}
jwt secret file path: {}
rpc client timeout: {}
taiko anchor address: {}
handover window slots: {}
handover start buffer: {}ms
l1 height lag: {}
max bytes size of batch: {}
max blocks per batch: {}
max time shift between blocks: {}
max_anchor_height_offset: {}
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
            config.validator_index,
            config.enable_preconfirmation,
            config.jwt_secret_file_path,
            config.rpc_client_timeout.as_secs(),
            config.taiko_anchor_address,
            config.handover_window_slots,
            config.handover_start_buffer_ms,
            config.l1_height_lag,
            config.max_bytes_size_of_batch,
            config.max_blocks_per_batch,
            config.max_time_shift_between_blocks_sec,
            config.max_anchor_height_offset,
        );

        config
    }
}
