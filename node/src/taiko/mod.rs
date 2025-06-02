#![allow(unused)] // TODO: remove this once using new rpc functions

use crate::{
    ethereum_l1::EthereumL1,
    metrics::Metrics,
    shared::{
        l2_block::L2Block,
        l2_slot_info::L2SlotInfo,
        l2_tx_lists::{self, PreBuiltTxList},
    },
    utils::{
        rpc_client::{HttpRPCClient, JSONRPCClient},
        types::*,
    },
};
use alloy::{
    consensus::{
        BlockHeader, SignableTransaction, Transaction as AnchorTransaction, TxEnvelope,
        transaction::Recovered,
    },
    contract::Error as ContractError,
    eips::BlockNumberOrTag,
    network::{Ethereum, EthereumWallet, NetworkWallet, TransactionBuilder},
    primitives::{Address, B256, BlockNumber},
    providers::{
        Identity, Provider, ProviderBuilder, RootProvider, WsConnect,
        fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
    },
    rpc::types::{Block as RpcBlock, BlockTransactionsKind, Transaction},
    signers::{
        Signature, SignerSync,
        local::{LocalSigner, PrivateKeySigner},
    },
    transports::TransportErrorKind,
};
use alloy_json_rpc::RpcError;
use anyhow::Error;
use config::{
    GOLDEN_TOUCH_ADDRESS, GOLDEN_TOUCH_PRIVATE_KEY, OperationType, TaikoConfig, WsProvider,
};
use ecdsa::SigningKey;
use k256::Secp256k1;
use l2_contracts_bindings::{LibSharedData, TaikoAnchor};
use serde_json::Value;
use std::{
    cmp::{max, min},
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use tokio::sync::RwLock;
use tracing::{debug, info, trace, warn};

pub mod config;
mod fixed_k_signer_chainbound;
mod l2_contracts_bindings;
pub mod preconf_blocks;
pub mod taiko_blob;
mod taiko_blob_coder;

pub struct Taiko {
    taiko_geth_provider_ws: RwLock<WsProvider>,
    taiko_geth_auth_rpc: JSONRPCClient,
    driver_rpc: HttpRPCClient,
    pub chain_id: u64,
    ethereum_l1: Arc<EthereumL1>,
    taiko_anchor: RwLock<TaikoAnchor::TaikoAnchorInstance<WsProvider>>,
    metrics: Arc<Metrics>,
    config: TaikoConfig,
}

impl Taiko {
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        ethereum_l1: Arc<EthereumL1>,
        metrics: Arc<Metrics>,
        taiko_config: TaikoConfig,
    ) -> Result<Self, Error> {
        let ws = WsConnect::new(taiko_config.taiko_geth_ws_url.to_string());
        let provider_ws = RwLock::new(
            ProviderBuilder::new()
                .connect_ws(ws.clone())
                .await
                .map_err(|e| {
                    anyhow::anyhow!("Taiko::new: Failed to create WebSocket provider: {e}")
                })?,
        );

        let chain_id = provider_ws.read().await.get_chain_id().await?;
        info!("L2 Chain ID: {}", chain_id);

        let taiko_anchor_address = Address::from_str(&taiko_config.taiko_anchor_address)?;
        let mut taiko_anchor = RwLock::new(TaikoAnchor::new(
            taiko_anchor_address,
            provider_ws.read().await.clone(),
        ));

        Ok(Self {
            taiko_geth_provider_ws: provider_ws,
            taiko_geth_auth_rpc: JSONRPCClient::new_with_timeout_and_jwt(
                &taiko_config.taiko_geth_auth_url,
                taiko_config.rpc_short_timeout,
                &taiko_config.jwt_secret_bytes,
            )?,
            driver_rpc: HttpRPCClient::new_with_jwt(
                &taiko_config.driver_url,
                taiko_config.rpc_short_timeout,
                &taiko_config.jwt_secret_bytes,
            )?,
            chain_id,
            ethereum_l1,
            taiko_anchor,
            metrics,
            config: taiko_config,
        })
    }

    pub async fn get_pending_l2_tx_list_from_taiko_geth(
        &self,
        base_fee: u64,
        batches_ready_to_send: u64,
    ) -> Result<Option<PreBuiltTxList>, Error> {
        let max_bytes_per_tx_list = calculate_max_bytes_per_tx_list(
            self.config.max_bytes_per_tx_list,
            self.config.throttling_factor,
            batches_ready_to_send,
            self.config.min_bytes_per_tx_list,
        );
        let params = vec![
            Value::String(format!("0x{}", hex::encode(self.config.preconfer_address))), // beneficiary address
            Value::from(base_fee),                                                      // baseFee
            Value::Number(
                self.ethereum_l1
                    .execution_layer
                    .get_config_block_max_gas_limit()
                    .into(),
            ), // blockMaxGasLimit
            Value::Number(max_bytes_per_tx_list.into()), // maxBytesPerTxList (128KB by default)
            Value::Array(vec![]),                        // locals (empty array)
            Value::Number(1.into()),                     // maxTransactionsLists
            Value::Number(0.into()),                     // minTip
        ];

        let result = self
            .taiko_geth_auth_rpc
            .call_method("taikoAuth_txPoolContentWithMinTip", params)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get L2 tx lists: {}", e))?;
        if result != Value::Null {
            let mut tx_lists = l2_tx_lists::decompose_pending_lists_json_from_geth(result)
                .map_err(|e| anyhow::anyhow!("Failed to decompose L2 tx lists: {}", e))?;
            // ignoring rest of tx lists, only one list per L2 block is processed
            Ok(Some(tx_lists.remove(0)))
        } else {
            Ok(None)
        }
    }

    fn print_number_of_received_txs(result: &l2_tx_lists::RPCReplyL2TxLists) {
        if let Some(tx_lists) = result.tx_lists.as_array() {
            let mut hashes = Vec::new();
            for tx_list in tx_lists {
                if let Some(tx_list_array) = tx_list.as_array() {
                    for tx in tx_list_array {
                        if let Some(hash) = tx.get("hash") {
                            hashes.push(hash.as_str().unwrap_or("").get(0..8).unwrap_or(""));
                        }
                    }
                }
            }
            tracing::debug!("Received L2 txs: [{}]", hashes.join(" "));
        }
    }

    pub async fn get_latest_l2_block_id(&self) -> Result<u64, Error> {
        let block_number = self
            .taiko_geth_provider_ws
            .read()
            .await
            .get_block_number()
            .await;

        self.check_for_ws_provider_failure(block_number, "Failed to get latest L2 block number")
            .await
    }

    pub async fn get_l2_block_by_number(
        &self,
        number: u64,
        full_txs: bool,
    ) -> Result<alloy::rpc::types::Block, Error> {
        let mut block_by_number = self
            .taiko_geth_provider_ws
            .read()
            .await
            .get_block_by_number(BlockNumberOrTag::Number(number));

        if full_txs {
            block_by_number = block_by_number.full();
        }

        let block = self
            .check_for_ws_provider_failure(
                block_by_number.await,
                "Failed to get L2 block by number",
            )
            .await?
            .ok_or_else(|| anyhow::anyhow!("Failed to get L2 block {}: value was None", number))?;
        Ok(block)
    }

    pub async fn fetch_l2_blocks_until_latest(
        &self,
        start_block: u64,
        full_txs: bool,
    ) -> Result<Vec<alloy::rpc::types::Block>, Error> {
        let start_time = std::time::Instant::now();
        let end_block = self.get_latest_l2_block_id().await?;
        let mut blocks = Vec::with_capacity(usize::try_from(end_block - start_block + 1)?);
        for block_number in start_block..=end_block {
            let block = self.get_l2_block_by_number(block_number, full_txs).await?;
            blocks.push(block);
        }
        debug!(
            "Fetched L2 blocks from {} to {} in {} ms",
            start_block,
            end_block,
            start_time.elapsed().as_millis()
        );
        Ok(blocks)
    }

    pub async fn get_transaction_by_hash(
        &self,
        hash: B256,
    ) -> Result<alloy::rpc::types::Transaction, Error> {
        let transaction_by_hash = self
            .taiko_geth_provider_ws
            .read()
            .await
            .get_transaction_by_hash(hash)
            .await;

        let transaction = self
            .check_for_ws_provider_failure(
                transaction_by_hash,
                "Failed to get L2 transaction by hash",
            )
            .await?
            .ok_or(anyhow::anyhow!(
                "Failed to get L2 transaction: value is None"
            ))?;
        Ok(transaction)
    }

    pub async fn get_latest_l2_block_id_hash_and_gas_used(
        &self,
    ) -> Result<(u64, B256, u64), Error> {
        self.get_l2_block_id_hash_and_gas_used(BlockNumberOrTag::Latest)
            .await
    }

    pub async fn get_l2_block_id_hash_and_gas_used(
        &self,
        block: BlockNumberOrTag,
    ) -> Result<(u64, B256, u64), Error> {
        let block = self.get_l2_block_header(block).await?;

        Ok((
            block.header.number(),
            block.header.hash,
            block.header.gas_used(),
        ))
    }

    pub async fn get_l2_block_hash(&self, number: u64) -> Result<B256, Error> {
        let block = self
            .get_l2_block_header(BlockNumberOrTag::Number(number))
            .await?;
        Ok(block.header.hash)
    }

    async fn get_l2_block_header(&self, block: BlockNumberOrTag) -> Result<RpcBlock, Error> {
        let block_by_number = self
            .taiko_geth_provider_ws
            .read()
            .await
            .get_block_by_number(block)
            .await;

        self.check_for_ws_provider_failure(block_by_number, "Failed to get latest L2 block")
            .await?
            .ok_or(anyhow::anyhow!("Failed to get latest L2 block"))
    }

    async fn get_latest_l2_block_with_txs(&self) -> Result<RpcBlock, Error> {
        let block_by_number = self
            .taiko_geth_provider_ws
            .read()
            .await
            .get_block_by_number(BlockNumberOrTag::Latest)
            .full()
            .await;

        self.check_for_ws_provider_failure(block_by_number, "Failed to get latest L2 block")
            .await?
            .ok_or(anyhow::anyhow!("Failed to get latest L2 block"))
    }

    pub async fn get_l2_slot_info(&self) -> Result<L2SlotInfo, Error> {
        self.get_l2_slot_info_by_parent_block(BlockNumberOrTag::Latest)
            .await
    }

    pub async fn get_l2_slot_info_by_parent_block(
        &self,
        block: BlockNumberOrTag,
    ) -> Result<L2SlotInfo, Error> {
        let l2_slot_timestamp = self.ethereum_l1.slot_clock.get_l2_slot_begin_timestamp()?;
        let (parent_id, parent_hash, parent_gas_used) =
            self.get_l2_block_id_hash_and_gas_used(block).await?;

        // Safe conversion with overflow check
        let parent_gas_used_u32 = u32::try_from(parent_gas_used).map_err(|_| {
            anyhow::anyhow!("parent_gas_used {} exceeds u32 max value", parent_gas_used)
        })?;

        let base_fee_config = self.get_base_fee_config();

        let base_fee = self
            .get_base_fee(
                parent_hash,
                parent_gas_used_u32,
                base_fee_config,
                l2_slot_timestamp,
            )
            .await?;

        debug!(
            timestamp = %l2_slot_timestamp,
            parent_hash = %parent_hash,
            parent_gas_used = %parent_gas_used_u32,
            base_fee = %base_fee,
            "L2 slot info"
        );

        Ok(L2SlotInfo::new(
            base_fee,
            l2_slot_timestamp,
            parent_id,
            parent_hash,
            parent_gas_used_u32,
        ))
    }

    /// Warning: be sure not to `read` from the rwlock
    /// while passing parameters to this function
    async fn check_for_ws_provider_failure<T>(
        &self,
        result: Result<T, RpcError<TransportErrorKind>>,
        error_message: &str,
    ) -> Result<T, Error> {
        match result {
            Ok(result) => Ok(result),
            Err(e) => {
                let check = self.recreate_ws_provider().await;
                Err(anyhow::anyhow!(
                    "{}. Recreating WebSocket provider. Transport error: {}",
                    error_message,
                    e
                ))
            }
        }
    }

    /// Warning: be sure not to `read` from the rwlock
    /// while passing parameters to this function
    async fn check_for_contract_failure<T>(
        &self,
        result: Result<T, ContractError>,
        error_message: &str,
    ) -> Result<T, Error> {
        match result {
            Ok(result) => Ok(result),
            Err(ContractError::TransportError(e)) => {
                let check = self.recreate_ws_provider().await;
                Err(anyhow::anyhow!(
                    "{}. Recreating WebSocket provider. Transport error: {}",
                    error_message,
                    e
                ))
            }
            Err(e) => Err(anyhow::anyhow!("{}: {}", error_message, e)),
        }
    }

    async fn recreate_ws_provider(&self) -> Result<(), Error> {
        let ws = WsConnect::new(self.config.taiko_geth_ws_url.clone());
        let provider = ProviderBuilder::new()
            .connect_ws(ws.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Taiko::new: Failed to create WebSocket provider: {e}"))?;

        *self.taiko_anchor.write().await = TaikoAnchor::new(
            Address::from_str(&self.config.taiko_anchor_address)?,
            provider.clone(),
        );
        *self.taiko_geth_provider_ws.write().await = provider;
        debug!(
            "Created new WebSocket provider for {}",
            self.config.taiko_geth_ws_url
        );
        Ok(())
    }

    pub async fn advance_head_to_new_l2_block(
        &self,
        l2_block: L2Block,
        anchor_origin_height: u64,
        l2_slot_info: L2SlotInfo,
        end_of_sequencing: bool,
        operation_type: OperationType,
    ) -> Result<Option<preconf_blocks::BuildPreconfBlockResponse>, Error> {
        tracing::debug!("Submitting new L2 blocks to the Taiko driver");

        let anchor_block_state_root = self
            .ethereum_l1
            .execution_layer
            .get_block_state_root_by_number(anchor_origin_height)
            .await?;

        let base_fee_config = self.get_base_fee_config();
        let sharing_pctg = base_fee_config.sharingPctg;

        debug!("processing {} txs", l2_block.prebuilt_tx_list.tx_list.len());

        let anchor_tx = self
            .construct_anchor_tx(
                *l2_slot_info.parent_hash(),
                anchor_origin_height,
                anchor_block_state_root,
                l2_slot_info.parent_gas_used(),
                base_fee_config.clone(),
                l2_slot_info.base_fee(),
            )
            .await?;
        let tx_list = std::iter::once(anchor_tx)
            .chain(l2_block.prebuilt_tx_list.tx_list.into_iter())
            .collect::<Vec<_>>();

        let tx_list_bytes = l2_tx_lists::encode_and_compress(&tx_list)?;
        let extra_data = vec![sharing_pctg];

        let executable_data = preconf_blocks::ExecutableData {
            base_fee_per_gas: l2_slot_info.base_fee(),
            block_number: l2_slot_info.parent_id() + 1,
            extra_data: format!("0x{:0>64}", hex::encode(extra_data)),
            fee_recipient: format!("0x{}", hex::encode(self.config.preconfer_address)),
            gas_limit: 241_000_000u64,
            parent_hash: format!("0x{}", hex::encode(l2_slot_info.parent_hash())),
            timestamp: l2_block.timestamp_sec,
            transactions: format!("0x{}", hex::encode(tx_list_bytes)),
        };

        let request_body = preconf_blocks::BuildPreconfBlockRequestBody {
            executable_data,
            end_of_sequencing,
        };

        const API_ENDPOINT: &str = "preconfBlocks";

        let response = match operation_type {
            OperationType::Preconfirm => {
                self.call_driver_short_timeout(http::Method::POST, API_ENDPOINT, &request_body)
                    .await?
            }
            OperationType::Reanchor => {
                self.call_driver_long_timeout(http::Method::POST, API_ENDPOINT, &request_body)
                    .await?
            }
        };

        trace!("Response from preconfBlocks: {:?}", response);

        let preconfirmed_block =
            preconf_blocks::BuildPreconfBlockResponse::new_from_value(response);

        if preconfirmed_block.is_none() {
            tracing::error!("Block was preconfirmed, but failed to decode response from driver.");
        }

        self.metrics.inc_blocks_preconfirmed();

        Ok(preconfirmed_block)
    }

    pub async fn get_status(&self) -> Result<preconf_blocks::TaikoStatus, Error> {
        trace!("Get status form taiko driver");

        const API_ENDPOINT: &str = "status";
        let request_body = serde_json::json!({});

        let response = self
            .call_driver_short_timeout(http::Method::GET, API_ENDPOINT, &request_body)
            .await?;

        trace!("Response from taiko status: {:?}", response);

        let status: preconf_blocks::TaikoStatus = serde_json::from_value(response)?;

        Ok(status)
    }

    async fn call_driver_short_timeout<T>(
        &self,
        method: http::Method,
        endpoint: &str,
        payload: &T,
    ) -> Result<Value, Error>
    where
        T: serde::Serialize,
    {
        let heartbeat_ms = self.ethereum_l1.slot_clock.get_preconf_heartbeat_ms();
        let max_duration = Duration::from_millis(heartbeat_ms / 2); // half of the heartbeat duration, leave time for other operations

        self.driver_rpc
            .retry_request_with_timeout(method, endpoint, payload, max_duration)
            .await
    }

    async fn call_driver_long_timeout<T>(
        &self,
        method: http::Method,
        endpoint: &str,
        payload: &T,
    ) -> Result<Value, Error>
    where
        T: serde::Serialize,
    {
        let driver_rpc = HttpRPCClient::new_with_jwt(
            &self.config.driver_url,
            self.config.rpc_long_timeout,
            &self.config.jwt_secret_bytes,
        )?;

        driver_rpc
            .retry_request_with_timeout(
                method,
                endpoint,
                payload,
                self.ethereum_l1.slot_clock.get_epoch_duration() / 2,
            )
            .await
    }

    fn get_base_fee_config(&self) -> LibSharedData::BaseFeeConfig {
        let config = self.ethereum_l1.execution_layer.get_pacaya_config();
        LibSharedData::BaseFeeConfig {
            adjustmentQuotient: config.baseFeeConfig.adjustmentQuotient,
            sharingPctg: config.baseFeeConfig.sharingPctg,
            gasIssuancePerSecond: config.baseFeeConfig.gasIssuancePerSecond,
            minGasExcess: config.baseFeeConfig.minGasExcess,
            maxGasIssuancePerBlock: config.baseFeeConfig.maxGasIssuancePerBlock,
        }
    }

    async fn construct_anchor_tx(
        &self,
        parent_hash: B256,
        anchor_block_id: u64,
        anchor_state_root: B256,
        parent_gas_used: u32,
        base_fee_config: LibSharedData::BaseFeeConfig,
        base_fee: u64,
    ) -> Result<Transaction, Error> {
        // Create the contract call
        let taiko_anchor = self.taiko_anchor.read().await;
        let tx_count_result = self
            .taiko_geth_provider_ws
            .read()
            .await
            .get_transaction_count(GOLDEN_TOUCH_ADDRESS)
            .block_id(parent_hash.into())
            .await;
        let nonce = self
            .check_for_ws_provider_failure(tx_count_result, "Failed to get nonce")
            .await?;
        let call_builder = taiko_anchor
            .anchorV3(
                anchor_block_id,
                anchor_state_root,
                parent_gas_used,
                base_fee_config,
                vec![],
            )
            .gas(1_000_000) // value expected by Taiko
            .max_fee_per_gas(u128::from(base_fee)) // value expected by Taiko
            .max_priority_fee_per_gas(0) // value expected by Taiko
            .nonce(nonce)
            .chain_id(self.chain_id);

        let typed_tx = call_builder
            .into_transaction_request()
            .build_typed_tx()
            .map_err(|_| anyhow::anyhow!("AnchorTX: Failed to build typed transaction"))?;

        let tx_eip1559 = typed_tx
            .eip1559()
            .ok_or_else(|| anyhow::anyhow!("AnchorTX: Failed to extract EIP-1559 transaction"))?;

        let signature = self.sign_hash_deterministic(tx_eip1559.signature_hash())?;
        let sig_tx = tx_eip1559.clone().into_signed(signature);

        let tx_envelope = TxEnvelope::from(sig_tx);

        debug!("AnchorTX transaction hash: {}", tx_envelope.tx_hash());

        let tx = Transaction {
            inner: Recovered::new_unchecked(tx_envelope, GOLDEN_TOUCH_ADDRESS),
            block_hash: None,
            block_number: None,
            transaction_index: None,
            effective_gas_price: None,
        };
        Ok(tx)
    }

    pub fn decode_anchor_tx_data(data: &[u8]) -> Result<u64, Error> {
        let tx_data =
            <TaikoAnchor::anchorV3Call as alloy::sol_types::SolCall>::abi_decode_validate(data)?;
        Ok(tx_data._anchorBlockId)
    }

    fn sign_hash_deterministic(&self, hash: B256) -> Result<Signature, Error> {
        fixed_k_signer_chainbound::sign_hash_deterministic(GOLDEN_TOUCH_PRIVATE_KEY, hash)
    }

    pub async fn get_base_fee(
        &self,
        parent_hash: B256,
        parent_gas_used: u32,
        base_fee_config: LibSharedData::BaseFeeConfig,
        l2_slot_timestamp: u64,
    ) -> Result<u64, Error> {
        let base_fee_v2_result = self
            .taiko_anchor
            .read()
            .await
            .getBasefeeV2(parent_gas_used, l2_slot_timestamp, base_fee_config)
            .block(parent_hash.into())
            .call()
            .await;
        let base_fee = self
            .check_for_contract_failure(base_fee_v2_result, "Failed to get base fee")
            .await?
            .basefee_;

        base_fee
            .try_into()
            .map_err(|err| anyhow::anyhow!("Failed to convert base fee to u64: {}", err))
    }

    pub async fn get_last_synced_anchor_block_id_from_taiko_anchor(&self) -> Result<u64, Error> {
        let last_synced_block = self
            .taiko_anchor
            .read()
            .await
            .lastSyncedBlock()
            .call()
            .await;
        self.check_for_contract_failure(last_synced_block, "Failed to get last synced block")
            .await
    }

    pub async fn get_last_synced_anchor_block_id_from_geth(&self) -> Result<u64, Error> {
        let block = self.get_latest_l2_block_with_txs().await?;
        let (anchor_tx, txs) = match block.transactions.as_transactions() {
            Some(txs) => txs
                .split_first()
                .ok_or_else(|| anyhow::anyhow!("Cannot get anchor transaction from block"))?,
            None => return Err(anyhow::anyhow!("No transactions in block")),
        };

        Self::decode_anchor_tx_data(anchor_tx.input())
    }
}

pub trait PreconfDriver {
    async fn get_status(&self) -> Result<preconf_blocks::TaikoStatus, Error>;
    async fn get_latest_l2_block_id(&self) -> Result<u64, Error>;
}

impl PreconfDriver for Taiko {
    async fn get_status(&self) -> Result<preconf_blocks::TaikoStatus, Error> {
        Taiko::get_status(self).await
    }

    async fn get_latest_l2_block_id(&self) -> Result<u64, Error> {
        Taiko::get_latest_l2_block_id(self).await
    }
}

/// Calculate the max bytes per tx list based on the number of batches ready to send.
/// The max bytes per tx list is reduced exponentially by given factor.
fn calculate_max_bytes_per_tx_list(
    max_bytes_per_tx_list: u64,
    throttling_factor: u64,
    batches_ready_to_send: u64,
    min_bytes_per_tx_list: u64,
) -> u64 {
    let mut size = max_bytes_per_tx_list;
    for _ in 0..batches_ready_to_send {
        size = size.saturating_sub(size / throttling_factor);
    }
    size = min(max_bytes_per_tx_list, max(size, min_bytes_per_tx_list));
    if batches_ready_to_send > 0 {
        debug!("Reducing max bytes per tx list to {}", size);
    }
    size
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_calculate_max_bytes_per_tx_list() {
        let max_bytes = 1000; // 128KB
        let throttling_factor = 10;
        let min_value = 100;

        // Test with no throttling (attempt = 0)
        assert_eq!(
            calculate_max_bytes_per_tx_list(max_bytes, throttling_factor, 0, min_value),
            max_bytes
        );

        assert_eq!(
            calculate_max_bytes_per_tx_list(max_bytes, throttling_factor, 1, min_value),
            900
        );

        assert_eq!(
            calculate_max_bytes_per_tx_list(max_bytes, throttling_factor, 2, min_value),
            810
        );

        assert_eq!(
            calculate_max_bytes_per_tx_list(max_bytes, throttling_factor, 3, min_value),
            729
        );

        // Test with throttling factor greater than max_bytes
        assert_eq!(calculate_max_bytes_per_tx_list(100, 200, 1, min_value), 100);

        // Test with zero max_bytes
        assert_eq!(
            calculate_max_bytes_per_tx_list(0, throttling_factor, 1, min_value),
            0
        );

        // Test with min_value
        assert_eq!(
            calculate_max_bytes_per_tx_list(max_bytes, throttling_factor, 500, min_value),
            min_value
        );
    }
}

// #[cfg(test)]
// mod test {
//     use super::*;
//     use crate::utils::rpc_server::test::RpcServer;
//     use std::net::SocketAddr;

//     #[tokio::test]
//     async fn test_get_pending_l2_tx_lists() {
//         let (mut rpc_server, taiko) = setup_rpc_server_and_taiko(3030).await;
//         let json = taiko
//             .get_pending_l2_tx_lists_from_taiko_geth()
//             .await
//             .unwrap();

//         assert_eq!(json.len(), 1);
//         assert_eq!(json[0].tx_list.len(), 2);
//         rpc_server.stop().await;
//     }

//     #[tokio::test]
//     async fn test_advance_head_to_new_l2_block() {
//         let (mut rpc_server, taiko) = setup_rpc_server_and_taiko(3040).await;
//         let value = serde_json::json!({
//             "TxLists": [
//                 [
//                     {
//                         "type": "0x0",
//                         "chainId": "0x28c61",
//                         "nonce": "0x1",
//                         "to": "0xbfadd5365bb2890ad832038837115e60b71f7cbb",
//                         "gas": "0x267ac",
//                         "gasPrice": "0x5e76e0800",
//                         "maxPriorityFeePerGas": null,
//                         "maxFeePerGas": null,
//                         "value": "0x0",
//                         "input": "0x40d097c30000000000000000000000004cea2c7d358e313f5d0287c475f9ae943fe1a913",
//                         "v": "0x518e6",
//                         "r": "0xb22da5cdc4c091ec85d2dda9054aa497088e55bd9f0335f39864ae1c598dd35",
//                         "s": "0x6eee1bcfe6a1855e89dd23d40942c90a036f273159b4c4fd217d58169493f055",
//                         "hash": "0x7c76b9906579e54df54fe77ad1706c47aca706b3eb5cfd8a30ccc3c5a19e8ecd"
//                     }
//                 ]
//             ]
//         });

//         let response = taiko.advance_head_to_new_l2_blocks(value).await.unwrap();
//         assert_eq!(
//             response["result"],
//             "Request received and processed successfully"
//         );
//         rpc_server.stop().await;
//     }

// async fn setup_rpc_server_and_taiko(port: u16) -> (RpcServer, Taiko) {
//     // Start the RPC server
//     let mut rpc_server = RpcServer::new();
//     let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
//     rpc_server.start_test_responses(addr).await.unwrap();

//     let taiko = Taiko::new(
//         &format!("ws://127.0.0.1:{}", port + 1),
//         &format!("http://127.0.0.1:{}", port),
//         &format!("http://127.0.0.1:{}", port + 2), // driver_url
//         1,
//         Duration::from_secs(10),
//         &[
//             0xa6, 0xea, 0x92, 0x58, 0xca, 0x91, 0x2c, 0x59, 0x3b, 0x3e, 0x36, 0xee, 0x36, 0xc1,
//             0x7f, 0xe9, 0x74, 0x47, 0xf9, 0x20, 0xf5, 0xb3, 0x6a, 0x90, 0x74, 0x4d, 0x79, 0xd4,
//             0xf2, 0xd6, 0xae, 0x62,
//         ],
//         PRECONFER_ADDRESS_ZERO,

//         "0x1670010000000000000000000000000000010001".to_string(),
//     )
//     .await
//     .unwrap();
//     (rpc_server, taiko)
// }
// }
