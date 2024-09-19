#[cfg(test)]
#[cfg(not(feature = "use_mock"))]
mod tests {
    use alloy::{
        node_bindings::Anvil,
        primitives::{Address, U256},
        providers::ProviderBuilder,
        sol,
        sol_types::SolValue,
    };

    use std::{borrow::Cow, process::Command, sync::Arc};

    sol! {
        #[allow(missing_docs)]
        #[sol(rpc)]
        contract IPreconfRegistry {
            struct Validator {
                address preconfer;
                uint40 startProposingAt;
                uint40 stopProposingAt;
            }
            function getPreconferAtIndex(uint256 index) external view returns (address);
            function getValidator(bytes32 pubKeyHash) external view returns (Validator memory);
        }
    }

    use crate::{
        bls::BLSService,
        ethereum_l1::EthereumL1,
        registration::Registration,
        utils::config::{AvsContractAddresses, ContractAddresses, EigenLayerContractAddresses},
    };
    fn get_contract_address(output: &Cow<'_, str>, contract_name: &str) -> String {
        output
            .lines()
            .find(|line| line.contains(contract_name))
            .map(|line| line.split_whitespace().last().unwrap())
            .unwrap()
            .to_string()
    }

    fn check_foundry_installed() -> bool {
        // Run `forge --version` to check if Foundry is installed
        let output = Command::new("forge").arg("--version").output().unwrap();

        return output.status.success();
    }

    #[tokio::test]
    async fn test() {
        tracing_subscriber::fmt()
            .with_env_filter("debug") // Set the log level
            .with_test_writer() // Ensure logs go to stdout during tests
            .init();

        // Check forge
        if !check_foundry_installed() {
            println!("Error: Foundry not installed!");
            return;
        }

        let anvil = Anvil::new().spawn();
        let rpc_url = anvil.endpoint();
        let ws_rpc_url = anvil.ws_endpoint();

        let private_key = anvil.keys()[2].clone();
        let user_address = Address::from_private_key(&private_key.clone().into());

        let pk_str = format!("0x{}", alloy::hex::encode(private_key.to_bytes()));
        // Deploy mock TaikoToken
        let output = Command::new("forge")
            .arg("script")
            .arg("scripts/deployment/mock/DeployMockTaikoToken.s.sol")
            .arg("--rpc-url")
            .arg(rpc_url.clone())
            .arg("--private-key")
            .arg(pk_str.clone())
            .arg("--broadcast")
            .arg("--skip-simulation")
            .current_dir("../SmartContracts/")
            .env("PRIVATE_KEY", pk_str.clone())
            .output()
            .unwrap();

        if !output.status.success() {
            println!("Forge script execution failed!");
            println!("Error: {}", String::from_utf8_lossy(&output.stderr));
            assert!(false);
        }

        let output = String::from_utf8_lossy(&output.stdout);
        let mock_taiko_token = get_contract_address(&output, "MockTaikoToken");

        // Deploy EigenlayerMVP
        let output = Command::new("forge")
            .arg("script")
            .arg("scripts/deployment/DeployEigenlayerMVP.s.sol")
            .arg("--rpc-url")
            .arg(rpc_url.clone())
            .arg("--private-key")
            .arg(pk_str.clone())
            .arg("--broadcast")
            .arg("--skip-simulation")
            .current_dir("../SmartContracts/")
            .env("PRIVATE_KEY", pk_str.clone())
            .output()
            .unwrap();

        if !output.status.success() {
            println!("Forge script execution failed!");
            println!("Error: {}", String::from_utf8_lossy(&output.stderr));
            assert!(false);
        }

        let output = String::from_utf8_lossy(&output.stdout);
        let avs_directory = get_contract_address(&output, "AVS Directory");
        let slasher = get_contract_address(&output, "Slasher");
        let strategy_manager = get_contract_address(&output, "Strategy Manager");

        let mock_address = anvil.addresses()[0].to_string();

        // Deploy AVS
        let output = Command::new("forge")
            .arg("script")
            .arg("scripts/deployment/mock/DeployMockAVS.s.sol")
            .arg("--rpc-url")
            .arg(rpc_url.clone())
            .arg("--private-key")
            .arg(pk_str.clone())
            .arg("--broadcast")
            .arg("--skip-simulation")
            .current_dir("../SmartContracts/")
            .env("PRIVATE_KEY", pk_str.clone())
            .env("AVS_DIRECTORY", avs_directory.clone())
            .env("SLASHER", slasher.clone())
            .env("TAIKO_L1", mock_address.clone())
            .env("TAIKO_TOKEN", mock_taiko_token)
            .env("BEACON_GENESIS_TIMESTAMP", "1725950369")
            .env(
                "BEACON_BLOCK_ROOT_CONTRACT",
                "0x000F3df6D732807Ef1319fB7B8bB8522d0Beac02",
            )
            .output()
            .unwrap();

        if !output.status.success() {
            println!("Forge script execution failed!");
            println!("Error: {}", String::from_utf8_lossy(&output.stderr));
            assert!(false);
        }

        let output = String::from_utf8_lossy(&output.stdout);
        let preconf_regestry = get_contract_address(&output, "Preconf Registry");
        let preconf_service_manager = get_contract_address(&output, "Preconf Service Manager");
        let preconf_task_manager = get_contract_address(&output, "Preconf Task Manager");

        // Create a new BLSService with a random private key
        let bls_service = Arc::new(BLSService::generate_key());

        // Create AVS contract addresses
        let avs_contracts = AvsContractAddresses {
            preconf_task_manager: preconf_task_manager,
            directory: avs_directory,
            service_manager: preconf_service_manager,
            preconf_registry: preconf_regestry.clone(),
        };

        // Create Eigenlayer contract addresses
        let eigen_layer = EigenLayerContractAddresses {
            strategy_manager: strategy_manager,
            slasher: slasher,
        };
        let contracts = ContractAddresses {
            taiko_l1: mock_address,
            eigen_layer,
            avs: avs_contracts,
        };

        let concensus_url_str = "https://docs-demo.quiknode.pro";
        // Create an Ethereum L1 client
        let eth = EthereumL1::new(
            &ws_rpc_url,
            &pk_str,
            &contracts,
            &concensus_url_str,
            12000,
            32,
            60,
            bls_service.clone(),
            1,
        )
        .await
        .unwrap();

        // Create a registration instance
        let registration = Registration::new(eth);

        // Register the preconfer
        if let Err(e) = registration.register().await {
            println!("Error find while registering: {}", e);
        }

        // Check if the preconfer is registered by PreconfRegistry.getPreconferAtIndex
        let rpc_url: reqwest::Url = rpc_url.parse().unwrap();
        let provider = ProviderBuilder::new().on_http(rpc_url.clone());
        let contract = IPreconfRegistry::new(preconf_regestry.parse().unwrap(), provider);
        assert!(
            contract
                .getPreconferAtIndex(U256::from(1))
                .call()
                .await
                .unwrap()
                ._0
                == user_address
        );

        // Add validator to registry
        if let Err(e) = registration.add_validator().await {
            println!("Error occurred while adding validator: {}", e);
        }

        // Copy logic form smart contract to get public key hash
        let pk_compressed = bls_service.get_public_key_compressed();
        let mut res_arr: [u8; 32] = [0; 32];
        res_arr[16..32].copy_from_slice(&pk_compressed[0..16]);
        let res1 = U256::from_be_bytes(res_arr);
        let mut res_arr: [u8; 32] = [0; 32];
        res_arr.copy_from_slice(&pk_compressed[16..48]);
        let res2 = U256::from_be_bytes(res_arr);
        let memory = [res1, res2];
        let encoded = memory.abi_encode_packed();
        let pub_key_hash = alloy::primitives::keccak256(encoded);

        // Get the validator from the PreconfRegistry
        let res = contract.getValidator(pub_key_hash).call().await.unwrap()._0;
        assert!(res.preconfer == user_address);
        assert!(true);
    }
}
