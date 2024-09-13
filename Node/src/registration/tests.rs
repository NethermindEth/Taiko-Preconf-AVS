#[cfg(test)]
#[cfg(not(feature = "use_mock"))]
mod tests {
    use alloy::{
        node_bindings::Anvil,
        primitives::{Address, U256},
        providers::ProviderBuilder,
        sol,
    };
    use std::{borrow::Cow, process::Command, sync::Arc};

    sol! {
        #[allow(missing_docs)]
        // solc v0.8.26; solc Counter.sol --via-ir --optimize --bin
        #[sol(rpc)]
        contract IPreconfRegistry {
            function getPreconferAtIndex(uint256 index) external view returns (address);
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
        // TODO check forge
        if !check_foundry_installed() {
            println!("Error: Foundry not installed!");
            return;
        }

        let anvil = Anvil::new().spawn();
        let anvil_url = anvil.endpoint();
        let rpc_url: reqwest::Url = anvil_url.parse().unwrap();

        let private_key = anvil.keys()[2].clone();
        let user_address = Address::from_private_key(&private_key.clone().into());

        let pk_str = format!("0x{}", alloy::hex::encode(private_key.to_bytes()));
        // Deploy mock TaikoToken
        let output = Command::new("forge")
            .arg("script")
            .arg("scripts/deployment/mock/DeployMockTaikoToken.s.sol")
            .arg("--rpc-url")
            .arg(rpc_url.to_string())
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
            .arg(rpc_url.to_string())
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
            .arg("scripts/deployment/DeployAVS.s.sol")
            .arg("--rpc-url")
            .arg(rpc_url.to_string())
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

        // Create a new BLSService with private key from Docker container
        let bls_service = Arc::new(BLSService::new(
            "0x14d50ac943d01069c206543a0bed3836f6062b35270607ebf1d1f238ceda26f1",
        ));

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
            eigen_layer,
            avs: avs_contracts,
        };

        let concensus_url_str = "https://docs-demo.quiknode.pro";
        // Create an Ethereum L1 client
        let eth = EthereumL1::new(
            &rpc_url.to_string(),
            &pk_str,
            &contracts,
            &concensus_url_str,
            12000,
            32,
            60,
            bls_service.clone(),
        )
        .await
        .unwrap();

        // Create a registration instance
        let registration = Registration::new(eth);

        // Register the preconfer
        if let Err(e) = registration.register().await {
            println!("Error find while registering: {}", e);
        }

        // TODO : Check if the preconfer is registered by PreconfRegistry.getPreconferAtIndex
        let provider = ProviderBuilder::new().on_http(rpc_url.clone());
        let contract = IPreconfRegistry::new(preconf_regestry.parse().unwrap(), provider);
        assert!(
            contract
                // TODO fix to 1 once contract is fixed
                .getPreconferAtIndex(U256::from(1))
                .call()
                .await
                .unwrap()
                ._0
                == user_address
        );

        assert!(true);
    }
}
