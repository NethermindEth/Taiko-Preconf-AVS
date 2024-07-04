use tracing::{info, warn};

pub struct Config {
    pub taiko_proposer_url: String,
    pub taiko_driver_url: String,
    pub ethereum_private_key: String,
    pub mev_boost_url: String,
    pub new_block_proposal_contract_address: String,
}

impl Config {
    pub fn read_env_variables() -> Self {
        const ETHEREUM_PRIVATE_KEY: &str = "ETHEREUM_PRIVATE_KEY";
        const NEW_BLOCK_PROPOSAL_CONTRACT_ADDRESS: &str = "NEW_BLOCK_PROPOSAL_CONTRACT_ADDRESS";

        let config = Self {
            taiko_proposer_url: std::env::var("TAIKO_PROPOSER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:1234".to_string()),
            taiko_driver_url: std::env::var("TAIKO_DRIVER_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:1235".to_string()),
            new_block_proposal_contract_address: std::env::var(NEW_BLOCK_PROPOSAL_CONTRACT_ADDRESS)
                .unwrap_or_else(|_| {
                    warn!(
                        "No new block proposal contract address found in {} env var, using default",
                        NEW_BLOCK_PROPOSAL_CONTRACT_ADDRESS
                    );
                    "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2".to_string()
                }),
            ethereum_private_key: std::env::var(ETHEREUM_PRIVATE_KEY).unwrap_or_else(|_| {
                warn!(
                    "No Ethereum private key found in {} env var, using default",
                    ETHEREUM_PRIVATE_KEY
                );
                "0x4c0883a69102937d6231471b5dbb6204fe512961708279f2e3e8a5d4b8e3e3e8".to_string()
            }),
            mev_boost_url: std::env::var("MEV_BOOST_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".to_string()),
        };

        info!(
            "\nConfiguration: \nTaiko proposer URL: {}, \nTaiko driver URL: {}, \nMEV Boost URL: {}, \nNew block proposal contract address: {}",
            config.taiko_proposer_url,
            config.taiko_driver_url,
            config.mev_boost_url,
            config.new_block_proposal_contract_address
        );

        config
    }
}
