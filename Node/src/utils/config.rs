use p2p_network::generate_secp256k1;
use p2p_network::network::P2PNetworkConfig;
use tracing::{info, warn};

pub struct Config {
    pub taiko_proposer_url: String,
    pub taiko_driver_url: String,
    pub avs_node_ecdsa_private_key: String,
    pub mev_boost_url: String,
    pub l1_beacon_url: String,
    pub l1_slot_duration_sec: u64,
    pub l1_slots_per_epoch: u64,
    pub l2_slot_duration_sec: u64,
    pub validator_bls_pubkey: String,
    pub block_proposed_receiver_timeout_sec: u64,
    pub preconf_registry_expiry_sec: u64,
    pub contract_addresses: ContractAddresses,
    pub p2p_network_config: P2PNetworkConfig,
    pub taiko_chain_id: u64,
    pub validator_index: u64,
}

#[derive(Debug)]
pub struct ContractAddresses {
    pub eigen_layer: EigenLayerContractAddresses,
    pub avs: AvsContractAddresses,
}

#[derive(Debug)]
pub struct EigenLayerContractAddresses {
    pub strategy_manager: String,
    pub slasher: String,
}

#[derive(Debug)]
pub struct AvsContractAddresses {
    pub preconf_task_manager: String,
    pub directory: String,
    pub service_manager: String,
    pub preconf_registry: String,
}

impl Config {
    pub fn read_env_variables() -> Self {
        // Load environment variables from .env file
        dotenv::dotenv().ok();

        let default_empty_address = "0x0000000000000000000000000000000000000000".to_string();

        const AVS_NODE_ECDSA_PRIVATE_KEY: &str = "AVS_NODE_ECDSA_PRIVATE_KEY";
        let avs_node_ecdsa_private_key = std::env::var(AVS_NODE_ECDSA_PRIVATE_KEY).unwrap_or({
            warn!(
                "No AVS node ECDSA private key found in {} env var, using default",
                AVS_NODE_ECDSA_PRIVATE_KEY
            );
            "0x4c0883a69102937d6231471b5dbb6204fe512961708279f2e3e8a5d4b8e3e3e8".to_string()
        });

        const AVS_PRECONF_TASK_MANAGER_CONTRACT_ADDRESS: &str =
            "AVS_PRECONF_TASK_MANAGER_CONTRACT_ADDRESS";
        let preconf_task_manager = std::env::var(AVS_PRECONF_TASK_MANAGER_CONTRACT_ADDRESS)
            .unwrap_or({
                warn!("No AVS preconf task manager contract address found in {} env var, using default", AVS_PRECONF_TASK_MANAGER_CONTRACT_ADDRESS);
                default_empty_address.clone()
            });

        const AVS_DIRECTORY_CONTRACT_ADDRESS: &str = "AVS_DIRECTORY_CONTRACT_ADDRESS";
        let directory = std::env::var(AVS_DIRECTORY_CONTRACT_ADDRESS).unwrap_or({
            warn!(
                "No AVS directory contract address found in {} env var, using default",
                AVS_DIRECTORY_CONTRACT_ADDRESS
            );
            default_empty_address.clone()
        });

        const AVS_SERVICE_MANAGER_CONTRACT_ADDRESS: &str = "AVS_SERVICE_MANAGER_CONTRACT_ADDRESS";
        let service_manager = std::env::var(AVS_SERVICE_MANAGER_CONTRACT_ADDRESS).unwrap_or({
            warn!(
                "No AVS service manager contract address found in {} env var, using default",
                AVS_SERVICE_MANAGER_CONTRACT_ADDRESS
            );
            default_empty_address.clone()
        });

        const AVS_PRECONF_REGISTRY_CONTRACT_ADDRESS: &str = "AVS_PRECONF_REGISTRY_CONTRACT_ADDRESS";
        let preconf_registry = std::env::var(AVS_PRECONF_REGISTRY_CONTRACT_ADDRESS).unwrap_or({
            warn!(
                "No AVS preconf registry contract address found in {} env var, using default",
                AVS_PRECONF_REGISTRY_CONTRACT_ADDRESS
            );
            default_empty_address.clone()
        });

        let avs = AvsContractAddresses {
            preconf_task_manager,
            directory,
            service_manager,
            preconf_registry,
        };

        const EIGEN_LAYER_STRATEGY_MANAGER_CONTRACT_ADDRESS: &str =
            "EIGEN_LAYER_STRATEGY_MANAGER_CONTRACT_ADDRESS";
        let strategy_manager = std::env::var(EIGEN_LAYER_STRATEGY_MANAGER_CONTRACT_ADDRESS).unwrap_or({
            warn!("No Eigen Layer strategy manager contract address found in {} env var, using default", EIGEN_LAYER_STRATEGY_MANAGER_CONTRACT_ADDRESS);
            default_empty_address.clone()
        });

        const EIGEN_LAYER_SLASHER_CONTRACT_ADDRESS: &str = "EIGEN_LAYER_SLASHER_CONTRACT_ADDRESS";
        let slasher = std::env::var(EIGEN_LAYER_SLASHER_CONTRACT_ADDRESS).unwrap_or({
            warn!(
                "No Eigen Layer slasher contract address found in {} env var, using default",
                EIGEN_LAYER_SLASHER_CONTRACT_ADDRESS
            );
            default_empty_address.clone()
        });

        let eigen_layer = EigenLayerContractAddresses {
            strategy_manager,
            slasher,
        };

        let contract_addresses = ContractAddresses { eigen_layer, avs };

        let l1_slot_duration_sec = std::env::var("L1_SLOT_DURATION_SEC")
            .unwrap_or("12".to_string())
            .parse::<u64>()
            .map(|val| {
                if val == 0 {
                    panic!("L1_SLOT_DURATION_SEC must be a positive number");
                }
                val
            })
            .expect("L1_SLOT_DURATION_SEC must be a number");

        let l1_slots_per_epoch = std::env::var("L1_SLOTS_PER_EPOCH")
            .unwrap_or("32".to_string())
            .parse::<u64>()
            .map(|val| {
                if val == 0 {
                    panic!("L1_SLOTS_PER_EPOCH must be a positive number");
                }
                val
            })
            .expect("L1_SLOTS_PER_EPOCH must be a number");

        let l2_slot_duration_sec = std::env::var("L2_SLOT_DURATION_SEC")
            .unwrap_or("3".to_string())
            .parse::<u64>()
            .map(|val| {
                if val == 0 {
                    panic!("L2_SLOT_DURATION_SEC must be a positive number");
                }
                val
            })
            .expect("L2_SLOT_DURATION_SEC must be a number");

        const VALIDATOR_PUBKEY: &str = "VALIDATOR_PUBKEY";
        let validator_pubkey = std::env::var(VALIDATOR_PUBKEY).unwrap_or({
            warn!(
                "No validator pubkey found in {} env var, using default",
                VALIDATOR_PUBKEY
            );
            "0x0".to_string()
        });

        let block_proposed_receiver_timeout_sec =
            std::env::var("BLOCK_PROPOSED_RECEIVER_TIMEOUT_SEC")
                .unwrap_or("120".to_string())
                .parse::<u64>()
                .expect("BLOCK_PROPOSED_RECEIVER_TIMEOUT_SEC must be a number");

        let preconf_registry_expiry_sec = std::env::var("PRECONF_REGISTRY_EXPIRY_SEC")
            .unwrap_or("3600".to_string())
            .parse::<u64>()
            .expect("PRECONF_REGISTRY_EXPIRY_SEC must be a number");

        // Load P2P config from env
        // Load Ipv4 address from env
        let address = std::env::var("ADDRESS").unwrap_or_else(|_| "0.0.0.0".to_string());
        let ipv4 = address.parse().unwrap();

        // Load boot node from env
        let boot_nodes: Option<Vec<String>> =
            if let Ok(bootnode_enr) = std::env::var("BOOTNODE_ENR") {
                Some(vec![bootnode_enr])
            } else {
                None
            };

        // Create P2P network config
        let p2p_network_config: P2PNetworkConfig = P2PNetworkConfig {
            local_key: generate_secp256k1(),
            listen_addr: "/ip4/0.0.0.0/tcp/9000".parse().unwrap(),
            ipv4,
            udpv4: 9000,
            tcpv4: 9000,
            boot_nodes,
        };

        let taiko_chain_id = std::env::var("TAIKO_CHAIN_ID")
            .expect("TAIKO_CHAIN_ID env variable must be set")
            .parse::<u64>()
            .map(|val| {
                if val == 0 {
                    panic!("TAIKO_CHAIN_ID must be a positive number");
                }
                val
            })
            .expect("TAIKO_CHAIN_ID must be a number");

        let validator_index = std::env::var("VALIDATOR_INDEX")
            .expect("VALIDATOR_INDEX env variable must be set")
            .parse::<u64>()
            .expect("VALIDATOR_INDEX must be a number");

        let config = Self {
            taiko_proposer_url: std::env::var("TAIKO_PROPOSER_URL")
                .unwrap_or("http://127.0.0.1:1234".to_string()),
            taiko_driver_url: std::env::var("TAIKO_DRIVER_URL")
                .unwrap_or("http://127.0.0.1:1235".to_string()),

            avs_node_ecdsa_private_key,
            mev_boost_url: std::env::var("MEV_BOOST_URL")
                .unwrap_or("http://127.0.0.1:8080".to_string()),
            l1_beacon_url: std::env::var("L1_BEACON_URL")
                .unwrap_or("http://127.0.0.1:4000".to_string()),
            l1_slot_duration_sec,
            l1_slots_per_epoch,
            l2_slot_duration_sec,
            validator_bls_pubkey: validator_pubkey,
            block_proposed_receiver_timeout_sec,
            preconf_registry_expiry_sec,
            contract_addresses,
            p2p_network_config,
            taiko_chain_id,
            validator_index,
        };

        info!(
            r#"
Configuration:
Taiko proposer URL: {},
Taiko driver URL: {},
MEV Boost URL: {},
Consensus layer URL: {}
L1 slot duration: {}
L1 slots per epoch: {}
L2 slot duration: {}
Validator pubkey: {}
Block proposed receiver timeout: {}
Preconf registry expiry seconds: {}
Contract addresses: {:#?}
p2p_network_config: {}
"#,
            config.taiko_proposer_url,
            config.taiko_driver_url,
            config.mev_boost_url,
            config.l1_beacon_url,
            config.l1_slot_duration_sec,
            config.l1_slots_per_epoch,
            config.l2_slot_duration_sec,
            config.validator_bls_pubkey,
            config.block_proposed_receiver_timeout_sec,
            config.preconf_registry_expiry_sec,
            config.contract_addresses,
            config.p2p_network_config,
        );

        config
    }
}
