use crate::{
    ethereum_l1::{
        l1_contracts_bindings::*, monitor_transaction::TransactionMonitor, ws_provider::WsProvider,
    },
    shared::{l2_block::L2Block, l2_tx_lists::encode_and_compress},
    utils::{config, types::*},
};
use alloy::{
    eips::BlockNumberOrTag,
    network::EthereumWallet,
    primitives::{Address, B256},
    providers::{Provider, ProviderBuilder, WsConnect},
    signers::local::PrivateKeySigner,
};
use anyhow::Error;
#[cfg(test)]
use mockall::automock;
use std::{str::FromStr, sync::Arc};

use tracing::debug;

use crate::ethereum_l1::propose_batch_builder::ProposeBatchBuilder;

use super::config::EthereumL1Config;

pub struct ExecutionLayer {
    provider_ws: Arc<WsProvider>,
    preconfer_address: Address,
    contract_addresses: ContractAddresses,
    pacaya_config: taiko_inbox::ITaikoInbox::Config,
    #[cfg(feature = "extra_gas_percentage")]
    extra_gas_percentage: u64,
    transaction_monitor: TransactionMonitor,
}

pub struct ContractAddresses {
    pub taiko_inbox: Address,
    pub taiko_token: Address,
    pub preconf_whitelist: Address,
    pub preconf_router: Address,
}

#[cfg_attr(test, allow(dead_code))]
#[cfg_attr(test, automock)]
impl ExecutionLayer {
    pub async fn new(config: EthereumL1Config) -> Result<Self, Error> {
        tracing::debug!(
            "Creating ExecutionLayer with WS URL: {}",
            config.execution_ws_rpc_url
        );

        let signer = PrivateKeySigner::from_str(&config.avs_node_ecdsa_private_key)?;
        let preconfer_address: Address = signer.address();
        tracing::info!("AVS node address: {}", preconfer_address);

        let wallet = EthereumWallet::from(signer);

        #[cfg(feature = "extra_gas_percentage")]
        let extra_gas_percentage = config.contract_addresses.extra_gas_percentage;

        let ws = WsConnect::new(config.execution_ws_rpc_url.to_string());

        let provider_ws: Arc<WsProvider> = Arc::new(
            ProviderBuilder::new()
                .wallet(wallet)
                .on_ws(ws.clone())
                .await
                .unwrap(),
        );

        let contract_addresses =
            Self::parse_contract_addresses(provider_ws.clone(), &config.contract_addresses)
                .await
                .map_err(|e| Error::msg(format!("Failed to parse contract addresses: {}", e)))?;

        let transaction_monitor = TransactionMonitor::new(
            provider_ws.clone(),
            config.min_priority_fee_per_gas_wei,
            config.tx_fees_increase_percentage,
            config.max_attempts_to_send_tx,
            config.delay_between_tx_attempts_sec,
        )
        .await?;

        let pacaya_config =
            Self::fetch_pacaya_config(&contract_addresses.taiko_inbox, &provider_ws).await?;

        Ok(Self {
            provider_ws: provider_ws,
            preconfer_address,
            contract_addresses,
            pacaya_config,
            #[cfg(feature = "extra_gas_percentage")]
            extra_gas_percentage,
            transaction_monitor,
        })
    }

    pub fn get_pacaya_config_max_blocks_per_batch(&self) -> u16 {
        self.pacaya_config.maxBlocksPerBatch
    }

    pub fn get_pacaya_config_max_anchor_height_offset(&self) -> u64 {
        self.pacaya_config.maxAnchorHeightOffset
    }

    pub fn get_preconfer_address(&self) -> PreconferAddress {
        self.preconfer_address.into_array()
    }

    async fn parse_contract_addresses(
        provider: Arc<WsProvider>,
        contract_addresses: &config::L1ContractAddresses,
    ) -> Result<ContractAddresses, Error> {
        let taiko_inbox = contract_addresses.taiko_inbox.parse()?;
        let preconf_whitelist = contract_addresses.preconf_whitelist.parse()?;
        let preconf_router = contract_addresses.preconf_router.parse()?;

        let contract = taiko_inbox::ITaikoInbox::new(taiko_inbox, provider);
        let taiko_token = contract
            .bondToken()
            .call()
            .await
            .map_err(|e| Error::msg(format!("Failed to get bond token: {}", e)))?
            ._0;

        Ok(ContractAddresses {
            taiko_inbox,
            taiko_token,
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

        tracing::info!(
            "📦 Proposing batch with {} blocks and {} bytes length",
            blocks.len(),
            tx_lists_bytes.len(),
        );

        let last_block_timestamp = l2_blocks
            .last()
            .ok_or(anyhow::anyhow!("No L2 blocks provided"))?
            .timestamp_sec;

        // Build proposeBatch transaction
        #[cfg(not(feature = "extra_gas_percentage"))]
        let builder = ProposeBatchBuilder::new(self.provider_ws.clone());
        #[cfg(feature = "extra_gas_percentage")]
        let builder = ProposeBatchBuilder::new(self.provider_ws.clone(), self.extra_gas_percentage);
        let tx = builder
            .build_propose_batch_tx(
                self.preconfer_address,
                self.contract_addresses.preconf_router,
                tx_lists_bytes,
                blocks.clone(),
                last_anchor_origin_height,
                last_block_timestamp,
            )
            .await?;

        let pending_nonce = self.get_preconfer_nonce_pending().await?;
        // Spawn a monitor for this transaction
        let _ = self
            .transaction_monitor
            .monitor_new_transaction(tx, pending_nonce)
            .await;

        Ok(())
    }

    async fn fetch_pacaya_config(
        taiko_inbox_address: &Address,
        ws_provider: &WsProvider,
    ) -> Result<taiko_inbox::ITaikoInbox::Config, Error> {
        let contract = taiko_inbox::ITaikoInbox::new(*taiko_inbox_address, ws_provider);
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

    pub async fn get_l1_height(&self) -> Result<u64, Error> {
        self.provider_ws
            .get_block_number()
            .await
            .map_err(|e| Error::msg(format!("Failed to get L1 height: {}", e)))
    }

    pub async fn get_block_state_root_by_number(&self, number: u64) -> Result<B256, Error> {
        let block = self
            .provider_ws
            .get_block_by_number(BlockNumberOrTag::Number(number))
            .await
            .map_err(|e| Error::msg(format!("Failed to get block by number ({number}): {}", e)))?
            .ok_or(anyhow::anyhow!("Failed to get block by number ({number})"))?;
        Ok(block.header.state_root)
    }

    pub async fn get_l2_height_from_taiko_inbox(&self) -> Result<u64, Error> {
        let contract = taiko_inbox::ITaikoInbox::new(
            self.contract_addresses.taiko_inbox.clone(),
            self.provider_ws.clone(),
        );
        let num_batches = contract.getStats2().call().await?._0.numBatches;
        // It is safe because num_batches initial value is 1
        let batch = contract.getBatch(num_batches - 1).call().await?.batch_;

        Ok(batch.lastBlockId)
    }

    pub async fn get_preconfer_nonce_latest(&self) -> Result<u64, Error> {
        let nonce_str: String = self
            .provider_ws
            .client()
            .request(
                "eth_getTransactionCount",
                (self.preconfer_address, "latest"),
            )
            .await
            .map_err(|e| Error::msg(format!("Failed to get nonce: {}", e)))?;

        u64::from_str_radix(nonce_str.trim_start_matches("0x"), 16)
            .map_err(|e| Error::msg(format!("Failed to convert nonce: {}", e)))
    }

    pub async fn get_preconfer_nonce_pending(&self) -> Result<u64, Error> {
        let nonce_str: String = self
            .provider_ws
            .client()
            .request(
                "eth_getTransactionCount",
                (self.preconfer_address, "pending"),
            )
            .await
            .map_err(|e| Error::msg(format!("Failed to get nonce: {}", e)))?;

        u64::from_str_radix(nonce_str.trim_start_matches("0x"), 16)
            .map_err(|e| Error::msg(format!("Failed to convert nonce: {}", e)))
    }

    pub async fn get_preconfer_inbox_bonds(&self) -> Result<alloy::primitives::U256, Error> {
        let contract =
            taiko_inbox::ITaikoInbox::new(self.contract_addresses.taiko_inbox, &self.provider_ws);
        let bonds_balance = contract
            .bondBalanceOf(self.preconfer_address)
            .call()
            .await
            .map_err(|e| Error::msg(format!("Failed to get bonds balance: {}", e)))?
            ._0;
        Ok(bonds_balance)
    }

    pub async fn get_preconfer_wallet_bonds(&self) -> Result<alloy::primitives::U256, Error> {
        let contract = IERC20::new(self.contract_addresses.taiko_token, &self.provider_ws);
        let allowance = contract
            .allowance(self.preconfer_address, self.contract_addresses.taiko_inbox)
            .call()
            .await
            .map_err(|e| Error::msg(format!("Failed to get allowance: {}", e)))?
            ._0;

        let balance = contract
            .balanceOf(self.preconfer_address)
            .call()
            .await
            .map_err(|e| Error::msg(format!("Failed to get preconfer balance: {}", e)))?
            ._0;

        Ok(balance.min(allowance))
    }

    #[cfg(test)]
    pub async fn new_from_pk(
        ws_rpc_url: String,
        private_key: elliptic_curve::SecretKey<k256::Secp256k1>,
    ) -> Result<Self, Error> {
        use super::l1_contracts_bindings::taiko_inbox::ITaikoInbox::ForkHeights;

        let signer = PrivateKeySigner::from_signing_key(private_key.into());
        let wallet = EthereumWallet::from(signer);

        let ws = WsConnect::new(ws_rpc_url.to_string());

        let provider_ws: Arc<WsProvider> = Arc::new(
            ProviderBuilder::new()
                .wallet(wallet)
                .on_ws(ws.clone())
                .await
                .unwrap(),
        );

        let preconfer_address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2" // some random address for test
            .parse()?;

        Ok(Self {
            provider_ws: provider_ws.clone(),
            preconfer_address,
            contract_addresses: ContractAddresses {
                taiko_inbox: Address::ZERO,
                taiko_token: Address::ZERO,
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
            #[cfg(feature = "extra_gas_percentage")]
            extra_gas_percentage: 5,
            transaction_monitor: TransactionMonitor::new(
                provider_ws.clone(),
                1000000000000000000,
                5,
                4,
                15,
            )
            .await
            .unwrap(),
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
        let ws_rpc_url = anvil.ws_endpoint();
        let private_key = anvil.keys()[0].clone();
        let el = ExecutionLayer::new_from_pk(ws_rpc_url, private_key)
            .await
            .unwrap();
        el.call_test_contract().await.unwrap();
    }
}
