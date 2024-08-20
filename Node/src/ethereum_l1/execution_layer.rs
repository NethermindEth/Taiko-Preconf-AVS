use super::slot_clock::SlotClock;
use crate::utils::config;
use alloy::{
    network::{Ethereum, EthereumWallet, NetworkWallet},
    primitives::{Address, Bytes, FixedBytes, U256},
    providers::ProviderBuilder,
    signers::{
        local::{LocalSigner, PrivateKeySigner},
        SignerSync,
    },
    sol,
    sol_types::SolValue,
};
use anyhow::Error;
use beacon_api_client::ProposerDuty;
use ecdsa::SigningKey;
use k256::Secp256k1;
use rand_core::{OsRng, RngCore};
use std::str::FromStr;
use std::sync::Arc;

pub struct ExecutionLayer {
    rpc_url: reqwest::Url,
    signer: LocalSigner<SigningKey<Secp256k1>>,
    wallet: EthereumWallet,
    avs_node_address: Address,
    contract_addresses: ContractAddresses,
    slot_clock: Arc<SlotClock>,
    preconf_registry_expiry_sec: u64,
}

pub struct ContractAddresses {
    pub eigen_layer: EigenLayerContractAddresses,
    pub avs: AvsContractAddresses,
}

pub struct EigenLayerContractAddresses {
    pub strategy_manager: Address,
    pub slasher: Address,
}

pub struct AvsContractAddresses {
    pub preconf_task_manager: Address,
    pub directory: Address,
    pub service_manager: Address,
    pub preconf_registry: Address,
}

pub struct Validator {
    // Preconfer that the validator proposer blocks for
    pub preconfer: [u8; 20],
    // Timestamp at which the preconfer may start proposing for the preconfer
    // 2 epochs from validator addition timestamp
    pub startProposingAt: u64,
    // Timestamp at which the preconfer must stop proposing for the preconfer
    // 2 epochs from validator removal timestamp
    pub stopProposingAt: u64,
}

sol!(
    #[allow(clippy::too_many_arguments)]
    #[allow(missing_docs)]
    #[sol(rpc)]
    PreconfTaskManager,
    "src/ethereum_l1/abi/PreconfTaskManager.json"
);

sol! {
    /// @dev Hook and it's data (currently used only during proposeBlock)
    struct HookCall {
        address hook;
        bytes data;
    }

    /// @dev Represents proposeBlock's _data input parameter
    struct BlockParams {
        address assignedProver; // DEPRECATED, value ignored.
        address coinbase;
        bytes32 extraData;
        bytes32 parentMetaHash;
        HookCall[] hookCalls; // DEPRECATED, value ignored.
        bytes signature;
        uint32 l1StateBlockNumber;
        uint64 timestamp;
    }
}

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    StrategyManager,
    "src/ethereum_l1/abi/StrategyManager.json"
);

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    Slasher,
    "src/ethereum_l1/abi/Slasher.json"
);

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    AVSDirectory,
    "src/ethereum_l1/abi/AVSDirectory.json"
);

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    PreconfRegistry,
    "src/ethereum_l1/abi/PreconfRegistry.json"
);

impl ExecutionLayer {
    pub fn new(
        rpc_url: &str,
        avs_node_ecdsa_private_key: &str,
        contract_addresses: &config::ContractAddresses,
        slot_clock: Arc<SlotClock>,
        preconf_registry_expiry_sec: u64,
    ) -> Result<Self, Error> {
        tracing::debug!("Creating ExecutionLayer with RPC URL: {}", rpc_url);

        let signer = PrivateKeySigner::from_str(avs_node_ecdsa_private_key)?;
        let avs_node_address: Address = signer.address();
        tracing::info!("AVS node address: {}", avs_node_address);

        let wallet = EthereumWallet::from(signer.clone());

        let contract_addresses = Self::parse_contract_addresses(contract_addresses)
            .map_err(|e| Error::msg(format!("Failed to parse contract addresses: {}", e)))?;

        Ok(Self {
            rpc_url: rpc_url.parse()?,
            signer,
            wallet,
            avs_node_address,
            contract_addresses,
            slot_clock,
            preconf_registry_expiry_sec,
        })
    }

    pub fn get_avs_node_address(&self) -> [u8; 20] {
        self.avs_node_address.into_array()
    }

    fn parse_contract_addresses(
        contract_addresses: &config::ContractAddresses,
    ) -> Result<ContractAddresses, Error> {
        let eigen_layer = EigenLayerContractAddresses {
            strategy_manager: contract_addresses.eigen_layer.strategy_manager.parse()?,
            slasher: contract_addresses.eigen_layer.slasher.parse()?,
        };

        let avs = AvsContractAddresses {
            preconf_task_manager: contract_addresses.avs.preconf_task_manager.parse()?,
            directory: contract_addresses.avs.directory.parse()?,
            service_manager: contract_addresses.avs.service_manager.parse()?,
            preconf_registry: contract_addresses.avs.preconf_registry.parse()?,
        };

        Ok(ContractAddresses { eigen_layer, avs })
    }

    pub async fn propose_new_block(
        &self,
        tx_list: Vec<u8>,
        parent_meta_hash: [u8; 32],
        lookahead_set: Vec<ProposerDuty>,
    ) -> Result<(), Error> {
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(self.wallet.clone())
            .on_http(self.rpc_url.clone());

        let contract =
            PreconfTaskManager::new(self.contract_addresses.avs.preconf_task_manager, provider);

        let block_params = BlockParams {
            assignedProver: Address::ZERO,
            coinbase: <EthereumWallet as NetworkWallet<Ethereum>>::default_signer_address(
                &self.wallet,
            ),
            extraData: FixedBytes::from(&[0u8; 32]),
            parentMetaHash: FixedBytes::from(&parent_meta_hash),
            hookCalls: vec![],
            signature: Bytes::from(vec![0; 32]),
            l1StateBlockNumber: 0,
            timestamp: 0,
        };

        let encoded_block_params = Bytes::from(BlockParams::abi_encode_sequence(&block_params));

        let tx_list = Bytes::from(tx_list);
        let lookahead_set_param = lookahead_set
            .iter()
            .map(|duty| {
                Ok(PreconfTaskManager::LookaheadSetParam {
                    timestamp: U256::from(self.slot_clock.start_of(duty.slot)?.as_millis()),
                    preconfer: Address::ZERO, //TODO: Replace it with a BLS key when the contract is ready.
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;

        let builder = contract.newBlockProposal(
            encoded_block_params,
            tx_list,
            U256::from(0), //TODO: Replace it with the proper lookaheadPointer when the contract is ready.
            lookahead_set_param,
        );

        let tx_hash = builder.send().await?.watch().await?;
        tracing::debug!("Proposed new block: {tx_hash}");

        Ok(())
    }

    pub async fn register(&self) -> Result<(), Error> {
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(self.wallet.clone())
            .on_http(self.rpc_url.clone());

        let strategy_manager = StrategyManager::new(
            self.contract_addresses.eigen_layer.strategy_manager,
            provider.clone(),
        );
        let tx_hash = strategy_manager
            .depositIntoStrategy(Address::ZERO, Address::ZERO, U256::from(1))
            .send()
            .await?
            .watch()
            .await?;
        tracing::debug!("Deposited into strategy: {tx_hash}");

        let slasher = Slasher::new(
            self.contract_addresses.eigen_layer.slasher,
            provider.clone(),
        );
        let tx_hash = slasher
            .optIntoSlashing(self.contract_addresses.avs.service_manager)
            .send()
            .await?
            .watch()
            .await?;
        tracing::debug!("Opted into slashing: {tx_hash}");

        let salt = Self::create_random_salt();
        let avs_directory =
            AVSDirectory::new(self.contract_addresses.avs.directory, provider.clone());
        let expiration_timestamp =
            U256::from(chrono::Utc::now().timestamp() as u64 + self.preconf_registry_expiry_sec);
        let digest_hash = avs_directory
            .calculateOperatorAVSRegistrationDigestHash(
                self.avs_node_address,
                self.contract_addresses.avs.service_manager,
                salt,
                expiration_timestamp,
            )
            .send()
            .await?
            .watch()
            .await?;

        let digest_hash_bytes = digest_hash.to_vec();

        // sign the digest hash with private key
        let signature = self.signer.sign_message_sync(&digest_hash_bytes)?;

        let signature_with_salt_and_expiry = PreconfRegistry::SignatureWithSaltAndExpiry {
            signature: Bytes::from(signature.as_bytes()),
            salt,
            expiry: expiration_timestamp,
        };

        let preconf_registry =
            PreconfRegistry::new(self.contract_addresses.avs.preconf_registry, provider);
        let tx_hash = preconf_registry
            .registerPreconfer(signature_with_salt_and_expiry)
            .send()
            .await?
            .watch()
            .await?;
        tracing::debug!("Registered preconfirming: {tx_hash}");

        Ok(())
    }

    fn create_random_salt() -> FixedBytes<32> {
        let mut salt: [u8; 32] = [0u8; 32];
        let mut os_rng = OsRng {};
        os_rng.fill_bytes(&mut salt);
        FixedBytes::from(&salt)
    }

    pub fn sign_message_with_private_ecdsa_key(&self, msg: &[u8]) -> Result<[u8; 65], Error> {
        let signature = self.signer.sign_message_sync(msg)?;
        Ok(signature.as_bytes())
    }

    pub async fn prove_incorrect_preconfirmation(
        &self,
        _block_id: u64,
        _tx_list_hash: [u8; 32],
        _signture: [u8; 65],
    ) -> Result<(), Error> {
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(self.wallet.clone())
            .on_http(self.rpc_url.clone());

        let _contract =
            PreconfTaskManager::new(self.contract_addresses.avs.preconf_task_manager, provider);
        // TODO: waiting for the new contract ABI
        // let builder = contract.proveIncorrectPreconfirmation(U256::from(block_id), tx_list_hash, signature);

        let tx_hash = FixedBytes::<32>::default(); // builder.send().await?.watch().await?;
        tracing::debug!("Proved incorrect preconfirmation: {tx_hash}");
        Ok(())
    }

    pub async fn get_validator(&self, pubkey: &[u8]) -> Result<Validator, Error> {
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(self.wallet.clone())
            .on_http(self.rpc_url.clone());
        let preconf_registry =
            PreconfRegistry::new(self.contract_addresses.avs.preconf_registry, provider);

        let pubkey: [u8; 32] = pubkey[..32].try_into()?;

        let validator = preconf_registry
            .getValidator(FixedBytes::from(pubkey))
            .call()
            .await?;

        Ok(Validator {
            preconfer: validator._0.preconfer.into_array(),
            startProposingAt: validator._0.startProposingAt,
            stopProposingAt: validator._0.stopProposingAt,
        })
    }

    #[cfg(test)]
    pub fn new_from_pk(
        rpc_url: reqwest::Url,
        private_key: elliptic_curve::SecretKey<k256::Secp256k1>,
    ) -> Result<Self, Error> {
        let signer = PrivateKeySigner::from_signing_key(private_key.into());
        let wallet = EthereumWallet::from(signer.clone());
        let clock = SlotClock::new(0u64, 0u64, 12u64, 32u64);

        Ok(Self {
            rpc_url,
            signer,
            wallet,
            avs_node_address: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2" // some random address for test
                .parse()?,
            slot_clock: Arc::new(clock),
            contract_addresses: ContractAddresses {
                eigen_layer: EigenLayerContractAddresses {
                    strategy_manager: Address::ZERO,
                    slasher: Address::ZERO,
                },
                avs: AvsContractAddresses {
                    preconf_task_manager: Address::ZERO,
                    directory: Address::ZERO,
                    service_manager: Address::ZERO,
                    preconf_registry: Address::ZERO,
                },
            },
            preconf_registry_expiry_sec: 120,
        })
    }

    #[cfg(test)]
    async fn call_test_contract(&self) -> Result<(), Error> {
        sol! {
            #[allow(missing_docs)]
            #[sol(rpc, bytecode="6080806040523460135760df908160198239f35b600080fdfe6080806040526004361015601257600080fd5b60003560e01c9081633fb5c1cb1460925781638381f58a146079575063d09de08a14603c57600080fd5b3460745760003660031901126074576000546000198114605e57600101600055005b634e487b7160e01b600052601160045260246000fd5b600080fd5b3460745760003660031901126074576020906000548152f35b34607457602036600319011260745760043560005500fea2646970667358221220e978270883b7baed10810c4079c941512e93a7ba1cd1108c781d4bc738d9090564736f6c634300081a0033")]
            contract Counter {
                uint256 public number;

                function setNumber(uint256 newNumber) public {
                    number = newNumber;
                }

                function increment() public {
                    number++;
                }
            }
        }

        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(self.wallet.clone())
            .on_http(self.rpc_url.clone());

        let contract = Counter::deploy(&provider).await?;

        let builder = contract.setNumber(U256::from(42));
        let tx_hash = builder.send().await?.watch().await?;
        println!("Set number to 42: {tx_hash}");

        let builder = contract.increment();
        let tx_hash = builder.send().await?.watch().await?;
        println!("Incremented number: {tx_hash}");

        let builder = contract.number();
        let number = builder.call().await?.number.to_string();

        assert_eq!(number, "43");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::node_bindings::Anvil;

    #[tokio::test]
    async fn test_call_contract() {
        // Ensure `anvil` is available in $PATH.
        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let private_key = anvil.keys()[0].clone();
        let el = ExecutionLayer::new_from_pk(rpc_url, private_key).unwrap();
        el.call_test_contract().await.unwrap();
    }

    #[tokio::test]
    async fn test_propose_new_block() {
        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let private_key = anvil.keys()[0].clone();
        let el = ExecutionLayer::new_from_pk(rpc_url, private_key).unwrap();

        el.propose_new_block(vec![0; 32], [0; 32], vec![])
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn test_register() {
        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let private_key = anvil.keys()[0].clone();
        let el = ExecutionLayer::new_from_pk(rpc_url, private_key).unwrap();

        let result = el.register().await;
        assert!(result.is_ok(), "Register method failed: {:?}", result.err());
    }
}
