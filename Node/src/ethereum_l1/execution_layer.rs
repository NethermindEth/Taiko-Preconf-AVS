use super::slot_clock::SlotClock;
use alloy::{
    network::{Ethereum, EthereumWallet, NetworkWallet},
    primitives::{Address, Bytes, FixedBytes, U256},
    providers::ProviderBuilder,
    signers::local::PrivateKeySigner,
    sol,
    sol_types::SolValue,
};
use anyhow::Error;
use beacon_api_client::ProposerDuty;
use std::rc::Rc;
use std::str::FromStr;

pub struct ExecutionLayer {
    rpc_url: reqwest::Url,
    wallet: EthereumWallet,
    taiko_preconfirming_address: Address,
    slot_clock: Rc<SlotClock>,
    avs_service_manager_contract_address: Address,
}

sol!(
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

impl ExecutionLayer {
    pub fn new(
        rpc_url: &str,
        private_key: &str,
        taiko_preconfirming_address: &str,
        slot_clock: Rc<SlotClock>,
        avs_service_manager_contract_address: &str,
    ) -> Result<Self, Error> {
        let signer = PrivateKeySigner::from_str(private_key)?;
        let wallet = EthereumWallet::from(signer);

        Ok(Self {
            rpc_url: rpc_url.parse()?,
            wallet,
            taiko_preconfirming_address: taiko_preconfirming_address.parse()?,
            slot_clock,
            avs_service_manager_contract_address: avs_service_manager_contract_address.parse()?,
        })
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

        let contract = PreconfTaskManager::new(self.taiko_preconfirming_address, provider);

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

        let strategy_manager =
            StrategyManager::new(self.taiko_preconfirming_address, provider.clone());
        let tx_hash = strategy_manager
            .depositIntoStrategy(Address::ZERO, Address::ZERO, U256::from(1))
            .send()
            .await?
            .watch()
            .await?;
        tracing::debug!("Deposited into strategy: {tx_hash}");

        let slasher = Slasher::new(self.taiko_preconfirming_address, provider);
        let tx_hash = slasher
            .optIntoSlashing(self.avs_service_manager_contract_address)
            .send()
            .await?
            .watch()
            .await?;
        tracing::debug!("Opted into slashing: {tx_hash}");

        

        Ok(())
    }

    #[cfg(test)]
    pub fn new_from_pk(
        rpc_url: reqwest::Url,
        private_key: elliptic_curve::SecretKey<k256::Secp256k1>,
    ) -> Result<Self, Error> {
        let signer = PrivateKeySigner::from_signing_key(private_key.into());
        let wallet = EthereumWallet::from(signer);
        let clock = SlotClock::new(0u64, 0u64, 12u64, 32u64);

        Ok(Self {
            rpc_url,
            wallet,
            taiko_preconfirming_address: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2" // some random address for test
                .parse()?,
            slot_clock: Rc::new(clock),
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
}
