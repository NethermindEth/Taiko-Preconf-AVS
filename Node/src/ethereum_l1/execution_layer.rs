use super::block_proposed::{EventSubscriptionBlockProposedV2, TaikoEvents};
use crate::{
    ethereum_l1::{l1_contracts_bindings::*, ws_provider::WsProvider},
    utils::{config, types::*},
};
use alloy::{
    consensus::{transaction::SignableTransaction, TypedTransaction},
    network::{Ethereum, EthereumWallet, NetworkWallet},
    primitives::{Address, Bytes, FixedBytes},
    providers::{Provider, ProviderBuilder, WsConnect},
    signers::{
        local::{LocalSigner, PrivateKeySigner},
        SignerSync,
    },
    sol_types::SolValue,
};
use anyhow::Error;
use ecdsa::SigningKey;
use k256::Secp256k1;
#[cfg(test)]
use mockall::automock;
use std::str::FromStr;
use tracing::debug;

pub struct ExecutionLayer {
    provider_ws: WsProvider,
    signer: LocalSigner<SigningKey<Secp256k1>>,
    wallet: EthereumWallet,
    preconfer_address: Address,
    contract_addresses: ContractAddresses,
    l1_chain_id: u64,
    pacaya_config: taiko_inbox::ITaikoInbox::Config,
}

pub struct ContractAddresses {
    pub taiko_l1: Address,
    pub preconf_whitelist: Address,
    pub preconf_router: Address,
    pub avs: AvsContractAddresses,
}

pub struct AvsContractAddresses {
    pub preconf_task_manager: Address,
}

#[cfg_attr(test, allow(dead_code))]
#[cfg_attr(test, automock)]
impl ExecutionLayer {
    pub async fn new(
        ws_rpc_url: &str,
        avs_node_ecdsa_private_key: &str,
        contract_addresses: &config::L1ContractAddresses,
    ) -> Result<Self, Error> {
        tracing::debug!("Creating ExecutionLayer with WS URL: {}", ws_rpc_url);

        let signer = PrivateKeySigner::from_str(avs_node_ecdsa_private_key)?;
        let preconfer_address: Address = signer.address();
        tracing::info!("AVS node address: {}", preconfer_address);

        let wallet = EthereumWallet::from(signer.clone());

        let contract_addresses = Self::parse_contract_addresses(contract_addresses)
            .map_err(|e| Error::msg(format!("Failed to parse contract addresses: {}", e)))?;

        let ws = WsConnect::new(ws_rpc_url.to_string());

        let provider_ws: WsProvider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .on_ws(ws.clone())
            .await
            .unwrap();

        let l1_chain_id = provider_ws.get_chain_id().await?;

        let pacaya_config =
            Self::fetch_pacaya_config(&contract_addresses.taiko_l1, &provider_ws).await?;

        Ok(Self {
            provider_ws,
            signer,
            wallet,
            preconfer_address,
            contract_addresses,
            l1_chain_id,
            pacaya_config,
        })
    }

    pub fn get_preconfer_address(&self) -> PreconferAddress {
        self.preconfer_address.into_array()
    }

    fn parse_contract_addresses(
        contract_addresses: &config::L1ContractAddresses,
    ) -> Result<ContractAddresses, Error> {
        let avs = AvsContractAddresses {
            preconf_task_manager: contract_addresses.avs.preconf_task_manager.parse()?,
        };

        let taiko_l1 = contract_addresses.taiko_l1.parse()?;
        let preconf_whitelist = contract_addresses.preconf_whitelist.parse()?;
        let preconf_router = contract_addresses.preconf_router.parse()?;

        Ok(ContractAddresses {
            taiko_l1,
            preconf_whitelist,
            preconf_router,
            avs,
        })
    }

    pub async fn get_operator_for_current_epoch(&self) -> Result<Address, Error> {
        let contract =
            PreconfWhitelist::new(self.contract_addresses.preconf_whitelist, &self.provider_ws);
        let operator = contract.getOperatorForCurrentEpoch().call().await?._0;
        Ok(operator)
    }

    pub async fn get_operator_for_next_epoch(&self) -> Result<Address, Error> {
        let contract =
            PreconfWhitelist::new(self.contract_addresses.preconf_whitelist, &self.provider_ws);
        let operator = contract.getOperatorForNextEpoch().call().await?._0;
        Ok(operator)
    }

    pub async fn propose_batch(
        &self,
        nonce: u64,
        tx_list: Vec<u8>,
        tx_count: u16,
    ) -> Result<Vec<u8>, Error> {
        let contract =
            PreconfRouter::new(self.contract_addresses.preconf_router, &self.provider_ws);

        let tx_list = Bytes::from(tx_list);

        let bytes_x = Bytes::new();

        let block_params = BlockParams {
            numTransactions: tx_count,
            timeShift: 0,
            signalSlots: vec![],
        };

        let batch_params = BatchParams {
            proposer: Address::ZERO,
            coinbase: <EthereumWallet as NetworkWallet<Ethereum>>::default_signer_address(
                &self.wallet,
            ),
            parentMetaHash: FixedBytes::from(&[0u8; 32]),
            anchorBlockId: 0,
            lastBlockTimestamp: 0,
            revertIfNotFirstProposal: false,
            blobParams: BlobParams {
                blobHashes: vec![],
                firstBlobIndex: 0,
                numBlobs: 0,
                byteOffset: 0,
                byteSize: 0,
            },
            blocks: vec![block_params],
        };

        let encoded_batch_params = Bytes::from(BatchParams::abi_encode_sequence(&batch_params));

        let propose_batch_wrapper = ProposeBatchWrapper {
            bytesX: bytes_x,
            bytesY: encoded_batch_params,
        };

        let encoded_propose_batch_wrapper = Bytes::from(ProposeBatchWrapper::abi_encode_sequence(
            &propose_batch_wrapper,
        ));
        // TODO check gas parameters
        let builder = contract
            .proposeBatch(encoded_propose_batch_wrapper, tx_list)
            .chain_id(self.l1_chain_id)
            .nonce(nonce)
            .gas(1_000_000)
            .max_fee_per_gas(20_000_000_000)
            .max_priority_fee_per_gas(1_000_000_000);

        // Build transaction
        let tx = builder.as_ref().clone().build_typed_tx();
        let Ok(TypedTransaction::Eip1559(mut tx)) = tx else {
            return Err(anyhow::anyhow!(
                "propose_new_block: Not EIP1559 transaction"
            ));
        };

        // Sign transaction
        let signature = self
            .wallet
            .default_signer()
            .sign_transaction(&mut tx)
            .await?;

        let mut encoded = Vec::new();
        tx.into_signed(signature).rlp_encode(&mut encoded);

        // Send transaction
        let pending = self.provider_ws.send_raw_transaction(&encoded).await?;
        tracing::debug!("Sending raw transaction, with hash {}", pending.tx_hash());

        Ok(encoded)
    }

    pub fn sign_message_with_private_ecdsa_key(&self, msg: &[u8]) -> Result<[u8; 65], Error> {
        let signature = self.signer.sign_message_sync(msg)?;
        Ok(signature.as_bytes())
    }

    pub async fn get_preconfer_nonce(&self) -> Result<u64, Error> {
        let nonce = self
            .provider_ws
            .get_transaction_count(self.preconfer_address)
            .await?;
        Ok(nonce)
    }

    pub async fn subscribe_to_block_proposed_event(
        &self,
    ) -> Result<EventSubscriptionBlockProposedV2, Error> {
        let taiko_events = TaikoEvents::new(self.contract_addresses.taiko_l1, &self.provider_ws);

        let block_proposed_filter = taiko_events.BlockProposedV2_filter().subscribe().await?;
        tracing::debug!("Subscribed to block proposed V2 event");

        Ok(EventSubscriptionBlockProposedV2(block_proposed_filter))
    }

    async fn fetch_pacaya_config(
        taiko_l1_address: &Address,
        ws_provider: &WsProvider,
    ) -> Result<taiko_inbox::ITaikoInbox::Config, Error> {
        let contract = taiko_inbox::ITaikoInbox::new(*taiko_l1_address, ws_provider);
        let pacaya_config = contract.pacayaConfig().call().await?._0;

        debug!(
            "Pacaya config: chainid {}, maxUnverifiedBatches {}, batchRingBufferSize {}",
            pacaya_config.chainId,
            pacaya_config.maxUnverifiedBatches,
            pacaya_config.batchRingBufferSize
        );

        Ok(pacaya_config)
    }

    pub fn get_pacaya_config(&self) -> taiko_inbox::ITaikoInbox::Config {
        self.pacaya_config.clone()
    }

    #[cfg(test)]
    pub async fn new_from_pk(
        ws_rpc_url: String,
        rpc_url: reqwest::Url,
        private_key: elliptic_curve::SecretKey<k256::Secp256k1>,
    ) -> Result<Self, Error> {
        use super::l1_contracts_bindings::taiko_inbox::ITaikoInbox::ForkHeights;

        let signer = PrivateKeySigner::from_signing_key(private_key.into());
        let wallet = EthereumWallet::from(signer.clone());

        let provider = ProviderBuilder::new().on_http(rpc_url.clone());
        let l1_chain_id = provider.get_chain_id().await?;

        let ws = WsConnect::new(ws_rpc_url.to_string());

        let provider_ws: WsProvider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .on_ws(ws.clone())
            .await
            .unwrap();

        Ok(Self {
            provider_ws,
            signer,
            wallet,
            preconfer_address: "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2" // some random address for test
                .parse()?,
            contract_addresses: ContractAddresses {
                taiko_l1: Address::ZERO,
                preconf_whitelist: Address::ZERO,
                preconf_router: Address::ZERO,
                avs: AvsContractAddresses {
                    preconf_task_manager: Address::ZERO,
                },
            },
            l1_chain_id,
            pacaya_config: taiko_inbox::ITaikoInbox::Config {
                chainId: 1,
                maxUnverifiedBatches: 100,
                batchRingBufferSize: 100,
                maxBatchesToVerify: 100,
                blockMaxGasLimit: 1000000000,
                livenessBondBase: alloy::primitives::Uint::from_limbs([1000000000000000000, 0]),
                livenessBondPerBlock: alloy::primitives::Uint::from_limbs([1000000000000000000, 0]),
                stateRootSyncInternal: 100,
                maxAnchorHeightOffset: 1000000000000000000,
                baseFeeConfig: taiko_inbox::LibSharedData::BaseFeeConfig {
                    adjustmentQuotient: 100,
                    sharingPctg: 100,
                    gasIssuancePerSecond: 1000000000,
                    minGasExcess: 1000000000000000000,
                    maxGasIssuancePerBlock: 1000000000,
                },
                provingWindow: 1000,
                cooldownWindow: alloy::primitives::Uint::from_limbs([1000000]),
                maxSignalsToReceive: 100,
                maxBlocksPerBatch: 1000,
                forkHeights: ForkHeights {
                    ontake: 0,
                    pacaya: 0,
                    shasta: 0,
                    unzen: 0,
                },
            },
        })
    }

    #[cfg(test)]
    async fn call_test_contract(&self) -> Result<(), Error> {
        alloy::sol! {
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

        let contract = Counter::deploy(&self.provider_ws).await?;

        let builder = contract.setNumber(alloy::primitives::U256::from(42));
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
        let ws_rpc_url = anvil.ws_endpoint();
        let private_key = anvil.keys()[0].clone();
        let el = ExecutionLayer::new_from_pk(ws_rpc_url, rpc_url, private_key)
            .await
            .unwrap();
        el.call_test_contract().await.unwrap();
    }
}
