use crate::{
    ethereum_l1::{l1_contracts_bindings::*, ws_provider::WsProvider},
    shared::{l2_block::L2Block, l2_tx_lists::encode_and_compress},
    utils::{config, types::*},
};
use alloy::{
    eips::BlockNumberOrTag,
    network::{
        Ethereum, EthereumWallet, NetworkWallet, TransactionBuilder, TransactionBuilder4844,
    },
    primitives::{Address, Bytes, FixedBytes, B256},
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::{BlockTransactionsKind, TransactionRequest},
    signers::local::PrivateKeySigner,
    sol_types::SolValue,
};
use anyhow::Error;
#[cfg(test)]
use mockall::automock;
use std::{str::FromStr, sync::Arc};

use crate::ethereum_l1::monitor_transaction::monitor_transaction;
use tracing::debug;

pub struct ExecutionLayer {
    provider_ws: Arc<WsProvider>,
    wallet: EthereumWallet,
    preconfer_address: Address,
    contract_addresses: ContractAddresses,
    pacaya_config: taiko_inbox::ITaikoInbox::Config,
}

pub struct ContractAddresses {
    pub taiko_l1: Address,
    pub preconf_whitelist: Address,
    pub preconf_router: Address,
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

        let pacaya_config =
            Self::fetch_pacaya_config(&contract_addresses.taiko_l1, &provider_ws).await?;

        Ok(Self {
            provider_ws: Arc::new(provider_ws),
            wallet,
            preconfer_address,
            contract_addresses,
            pacaya_config,
        })
    }

    pub fn get_preconfer_address(&self) -> PreconferAddress {
        self.preconfer_address.into_array()
    }

    fn parse_contract_addresses(
        contract_addresses: &config::L1ContractAddresses,
    ) -> Result<ContractAddresses, Error> {
        let taiko_l1 = contract_addresses.taiko_l1.parse()?;
        let preconf_whitelist = contract_addresses.preconf_whitelist.parse()?;
        let preconf_router = contract_addresses.preconf_router.parse()?;

        Ok(ContractAddresses {
            taiko_l1,
            preconf_whitelist,
            preconf_router,
        })
    }

    pub async fn get_operator_for_current_epoch(&self) -> Result<Address, Error> {
        let contract =
            PreconfWhitelist::new(self.contract_addresses.preconf_whitelist, &self.provider_ws);
        let operator = contract
            .getOperatorForCurrentEpoch()
            .call()
            .await
            .map_err(|e| Error::msg(format!("Failed to get operator for current epoch: {}", e)))?
            ._0;
        Ok(operator)
    }

    pub async fn is_operator_for_current_epoch(&self) -> Result<bool, Error> {
        let operator = self.get_operator_for_current_epoch().await?;
        Ok(operator == self.preconfer_address)
    }

    pub async fn get_operator_for_next_epoch(&self) -> Result<Address, Error> {
        let contract =
            PreconfWhitelist::new(self.contract_addresses.preconf_whitelist, &self.provider_ws);
        let operator = contract
            .getOperatorForNextEpoch()
            .call()
            .await
            .map_err(|e| Error::msg(format!("Failed to get operator for next epoch: {}", e)))?
            ._0;
        Ok(operator)
    }

    pub async fn is_operator_for_next_epoch(&self) -> Result<bool, Error> {
        let operator = self.get_operator_for_next_epoch().await?;
        Ok(operator == self.preconfer_address)
    }

    pub async fn send_batch_to_l1(
        &self,
        l2_blocks: Vec<L2Block>,
        last_anchor_origin_height: u64,
    ) -> Result<(), Error> {
        let mut tx_vec = Vec::new();
        let mut blocks = Vec::new();

        for (i, l2_block) in l2_blocks.iter().enumerate() {
            let count = l2_block.prebuilt_tx_list.tx_list.len() as u16;
            tx_vec.extend(l2_block.prebuilt_tx_list.tx_list.clone());

            /* times_shift is the difference in seconds between the current L2 block and the L2 previous block. */
            let time_shift: u8 = if i == 0 {
                /* For first block, we don't have a previous block to compare the timestamp with. */
                0
            } else {
                (l2_block.timestamp_sec - l2_blocks[i - 1].timestamp_sec)
                    .try_into()
                    .map_err(|e| Error::msg(format!("Failed to convert time shift to u8: {}", e)))?
            };
            blocks.push(BlockParams {
                numTransactions: count,
                timeShift: time_shift,
                signalSlots: vec![],
            });
        }

        let tx_lists_bytes = encode_and_compress(&tx_vec)?;

        tracing::debug!(
            "Proposing batch with {} bloks and {} bytes length",
            blocks.len(),
            tx_lists_bytes.len(),
        );

        let last_block_timestamp = l2_blocks
            .last()
            .ok_or(anyhow::anyhow!("No L2 blocks provided"))?
            .timestamp_sec;

        // TODO estimate gas and select blob or calldata transaction
        // Build eip4844 transaction
        let tx_blob = self
            .build_propose_batch_blob(
                &tx_lists_bytes,
                blocks.clone(),
                last_anchor_origin_height,
                last_block_timestamp,
            )
            .await?;
        let tx_blob_gas = self.provider_ws.estimate_gas(&tx_blob).await.unwrap_or(0);

        // Build eip1559 transaction
        let tx_calldata = self.build_propose_batch_calldata(
            tx_lists_bytes.clone(),
            blocks.clone(),
            last_anchor_origin_height,
            last_block_timestamp,
        ).await?;
        let tx_calldata_gas = self.provider_ws.estimate_gas(&tx_calldata).await.unwrap_or(0);

        // If no gas estimate, return error
        if tx_calldata_gas == 0 && tx_blob_gas == 0 {
            return Err(anyhow::anyhow!("Failed to estimate gas for both transaction types"));
        }

        tracing::debug!(
            "Calldata gas: {} Blob gas: {}",
            tx_calldata_gas,
            tx_blob_gas
        );

        // Select transaction to send
        // If price the same, choose calldata
        let (tx, gas_limit) = if tx_blob_gas < tx_calldata_gas {
            (tx_blob, tx_blob_gas)
        } else {
            (tx_calldata, tx_calldata_gas)
        };

        // Set tx gas limit
        let tx = tx.with_gas_limit(gas_limit);

        // Send transaction
        let pending_tx = self
            .provider_ws
            .send_transaction(tx)
            .await?
            .register()
            .await?;

        tracing::debug!(
            "Call proposeBatch with hash {}",
            pending_tx.tx_hash()
        );

        // Spawn a monitor for this transaction
        monitor_transaction(self.provider_ws.clone(), *pending_tx.tx_hash()).await;

        Ok(())
    }

    pub async fn build_propose_batch_calldata(
        &self,
        tx_list: Vec<u8>,
        blocks: Vec<BlockParams>,
        last_anchor_origin_height: u64,
        last_block_timestamp: u64,
    ) -> Result<TransactionRequest, Error> {
        let tx_list_len = tx_list.len() as u32;
        let tx_list = Bytes::from(tx_list);

        let bytes_x = Bytes::new();

        let batch_params = BatchParams {
            proposer: self.preconfer_address,
            coinbase: <EthereumWallet as NetworkWallet<Ethereum>>::default_signer_address(
                &self.wallet,
            ),
            parentMetaHash: FixedBytes::from(&[0u8; 32]),
            anchorBlockId: last_anchor_origin_height,
            lastBlockTimestamp: last_block_timestamp,
            revertIfNotFirstProposal: false,
            blobParams: BlobParams {
                blobHashes: vec![],
                firstBlobIndex: 0,
                numBlobs: 0,
                byteOffset: 0,
                byteSize: tx_list_len,
                createdIn: 0,
            },
            blocks,
        };

        let encoded_batch_params = Bytes::from(BatchParams::abi_encode(&batch_params));

        let propose_batch_wrapper = ProposeBatchWrapper {
            bytesX: bytes_x,
            bytesY: encoded_batch_params,
        };

        let encoded_propose_batch_wrapper = Bytes::from(ProposeBatchWrapper::abi_encode_sequence(
            &propose_batch_wrapper,
        ));

        let tx = TransactionRequest::default()
            .with_from(self.preconfer_address)
            .with_to(self.contract_addresses.preconf_router)
            .with_call(&PreconfRouter::proposeBatchCall {
                _params: encoded_propose_batch_wrapper,
                _txList: tx_list,
            });

        Ok(tx)
    }

    pub async fn build_propose_batch_blob(
        &self,
        tx_list: &Vec<u8>,
        blocks: Vec<BlockParams>,
        last_anchor_origin_height: u64,
        last_block_timestamp: u64,
    ) -> Result<TransactionRequest, Error> {
        let tx_list_len = tx_list.len() as u32;

        let bytes_x = Bytes::new();

        // Build sidecar
        let sidecar = crate::taiko::taiko_blob::build_taiko_blob_sidecar(tx_list)?;
        let num_blobs = sidecar.blobs.len() as u8;

        let batch_params = BatchParams {
            proposer: self.preconfer_address,
            coinbase: <EthereumWallet as NetworkWallet<Ethereum>>::default_signer_address(
                &self.wallet,
            ),
            parentMetaHash: FixedBytes::from(&[0u8; 32]),
            anchorBlockId: last_anchor_origin_height,
            lastBlockTimestamp: last_block_timestamp,
            revertIfNotFirstProposal: false,
            blobParams: BlobParams {
                blobHashes: vec![],
                firstBlobIndex: 0,
                numBlobs: num_blobs,
                byteOffset: 0,
                byteSize: tx_list_len,
                createdIn: 0,
            },
            blocks,
        };

        let encoded_batch_params = Bytes::from(BatchParams::abi_encode(&batch_params));

        let propose_batch_wrapper = ProposeBatchWrapper {
            bytesX: bytes_x,
            bytesY: encoded_batch_params,
        };

        let encoded_propose_batch_wrapper = Bytes::from(ProposeBatchWrapper::abi_encode_sequence(
            &propose_batch_wrapper,
        ));

        let tx = TransactionRequest::default()
            .with_from(self.preconfer_address)
            .with_to(self.contract_addresses.preconf_router)
            .with_blob_sidecar(sidecar)
            .with_call(&PreconfRouter::proposeBatchCall {
                _params: encoded_propose_batch_wrapper,
                _txList: Bytes::new(),
            });

        Ok(tx)
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

    pub async fn get_anchor_block_id(&self) -> Result<u64, Error> {
        let contract =
            taiko_inbox::ITaikoInbox::new(self.contract_addresses.taiko_l1, &self.provider_ws);
        let num_batches = contract.getStats2().call().await?._0.numBatches;
        let batch = contract.getBatch(num_batches - 1).call().await?.batch_;
        Ok(batch.anchorBlockId)
    }

    pub async fn get_l1_height(&self) -> Result<u64, Error> {
        self.provider_ws
            .get_block_number()
            .await
            .map_err(|e| Error::msg(format!("Failed to get L1 height: {}", e)))
    }

    pub async fn get_block_state_root_by_number(&self, number: u64) -> Result<B256, Error> {
        let block = self
            .provider_ws
            .get_block_by_number(
                BlockNumberOrTag::Number(number),
                BlockTransactionsKind::Hashes,
            )
            .await
            .map_err(|e| Error::msg(format!("Failed to get block by number: {}", e)))?
            .ok_or(anyhow::anyhow!("Failed to get latest L2 block"))?;
        Ok(block.header.state_root)
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

        let ws = WsConnect::new(ws_rpc_url.to_string());

        let provider_ws: WsProvider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .on_ws(ws.clone())
            .await
            .unwrap();

        let preconfer_address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2" // some random address for test
            .parse()?;

        Ok(Self {
            provider_ws: Arc::new(provider_ws),
            wallet,
            preconfer_address,
            contract_addresses: ContractAddresses {
                taiko_l1: Address::ZERO,
                preconf_whitelist: Address::ZERO,
                preconf_router: Address::ZERO,
            },
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
