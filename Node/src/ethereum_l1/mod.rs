#![allow(unused)] //TODO remove after the EthereumL1 is used in release code

use alloy::{
    network::EthereumWallet,
    primitives::{Address, Bytes, U256},
    providers::ProviderBuilder,
    signers::local::PrivateKeySigner,
    sol,
};
use anyhow::Error;
use std::str::FromStr;

pub struct EthereumL1 {
    rpc_url: reqwest::Url,
    wallet: EthereumWallet,
}

sol!(
    #[allow(missing_docs)]
    #[sol(rpc)]
    PreconfTaskManager,
    "src/ethereum_l1/abi/PreconfTaskManager.json"
);

impl EthereumL1 {
    pub fn new(rpc_url: &str, private_key: &str) -> Result<Self, Error> {
        let signer = PrivateKeySigner::from_str(private_key)?;
        let wallet = EthereumWallet::from(signer);

        Ok(Self {
            rpc_url: rpc_url.parse()?,
            wallet,
        })
    }

    pub async fn propose_new_block(&self, address: Address) -> Result<(), Error> {
        let provider = ProviderBuilder::new()
            .with_recommended_fillers()
            .wallet(self.wallet.clone())
            .on_http(self.rpc_url.clone());

        let contract = PreconfTaskManager::new(address, provider);
        let block_params = Bytes::from(vec![0; 32]);
        let tx_list = Bytes::from(vec![0; 32]);
        let lookahead_set_param: Vec<PreconfTaskManager::LookaheadSetParam> = Vec::new();
        let builder =
            contract.newBlockProposal(block_params, tx_list, U256::from(0), lookahead_set_param);
        let tx_hash = builder.send().await?.watch().await?;
        tracing::debug!("Proposed new block: {tx_hash}");

        Ok(())
    }

    #[cfg(test)]
    fn new_from_pk(
        rpc_url: reqwest::Url,
        private_key: elliptic_curve::SecretKey<k256::Secp256k1>,
    ) -> Result<Self, Error> {
        let signer = PrivateKeySigner::from_signing_key(private_key.into());
        let wallet = EthereumWallet::from(signer);

        Ok(Self { rpc_url, wallet })
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
        let address = contract.address().clone();

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
    use alloy::hex;
    use alloy::node_bindings::{Anvil, AnvilInstance};
    use alloy::providers::Provider;
    use alloy::rpc::types::TransactionRequest;

    #[tokio::test]
    async fn test_call_contract() {
        // Ensure `anvil` is available in $PATH.
        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let private_key = anvil.keys()[0].clone();
        let ethereum_l1 = EthereumL1::new_from_pk(rpc_url, private_key).unwrap();
        ethereum_l1.call_test_contract().await.unwrap();
    }

    #[tokio::test]
    async fn test_propose_new_block() {
        let anvil = Anvil::new().try_spawn().unwrap();
        let rpc_url: reqwest::Url = anvil.endpoint().parse().unwrap();
        let private_key = anvil.keys()[0].clone();
        let ethereum_l1 = EthereumL1::new_from_pk(rpc_url, private_key).unwrap();

        // some random address for test
        ethereum_l1
            .propose_new_block(
                "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
                    .parse()
                    .unwrap(),
            )
            .await
            .unwrap();
    }
}
