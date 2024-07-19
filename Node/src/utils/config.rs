use tracing::{info, warn};

pub struct Config {
    pub taiko_proposer_url: String,
    pub taiko_driver_url: String,
    pub ethereum_private_key: String,
    pub mev_boost_url: String,
    pub taiko_preconfirming_address: String,
    pub l1_beacon_url: String,
    pub l1_slot_duration_sec: u64,
}

impl Config {
    pub fn read_env_variables() -> Self {
        // Load environment variables from .env file
        dotenv::dotenv().ok();

        const ETHEREUM_PRIVATE_KEY: &str = "ETHEREUM_PRIVATE_KEY";
        const TAIKO_PRECONFIRMING_ADDRESS: &str = "TAIKO_PRECONFIRMING_ADDRESS";

        let l1_slot_duration_sec = std::env::var("L1_SLOT_DURATION_SEC")
            .unwrap_or_else(|_| "12".to_string())
            .parse::<u64>()
            .map(|val| {
                if val == 0 {
                    panic!("L1_SLOT_DURATION_SEC must be a positive number");
                }
                val
            })
            .expect("L1_SLOT_DURATION_SEC must be a number");

        let config = Self {
            taiko_proposer_url: std::env::var("TAIKO_PROPOSER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:1234".to_string()),
            taiko_driver_url: std::env::var("TAIKO_DRIVER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:1235".to_string()),
            taiko_preconfirming_address: std::env::var(TAIKO_PRECONFIRMING_ADDRESS).unwrap_or_else(
                |_| {
                    warn!(
                        "No new block proposal contract address found in {} env var, using default",
                        TAIKO_PRECONFIRMING_ADDRESS
                    );
                    "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string()
                },
            ),
            ethereum_private_key: std::env::var(ETHEREUM_PRIVATE_KEY).unwrap_or_else(|_| {
                warn!(
                    "No Ethereum private key found in {} env var, using default",
                    ETHEREUM_PRIVATE_KEY
                );
                "0x4c0883a69102937d6231471b5dbb6204fe512961708279f2e3e8a5d4b8e3e3e8".to_string()
            }),
            mev_boost_url: std::env::var("MEV_BOOST_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string()),
            l1_beacon_url: std::env::var("L1_BEACON_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:4000".to_string()),
            l1_slot_duration_sec,
        };

        info!(
            r#"
Configuration:
Taiko proposer URL: {},
Taiko driver URL: {},
MEV Boost URL: {},
New block proposal contract address: {}
Consensus layer URL: {}
L1 slot duration: {}
"#,
            config.taiko_proposer_url,
            config.taiko_driver_url,
            config.mev_boost_url,
            config.taiko_preconfirming_address,
            config.l1_beacon_url,
            config.l1_slot_duration_sec
        );

        config
    }
}
