#![allow(unused)] // TODO: remove this once using new rpc functions

use crate::{
    ethereum_l1::EthereumL1,
    utils::{
        rpc_client::{HttpRPCClient, JSONRPCClient},
        types::*,
    },
};
use alloy::{
    consensus::BlockHeader,
    eips::BlockNumberOrTag,
    network::{Ethereum, EthereumWallet, NetworkWallet, TransactionBuilder},
    primitives::{Address, BlockNumber, B256},
    providers::{
        fillers::{BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller},
        Identity, Provider, ProviderBuilder, RootProvider, WsConnect,
    },
    rpc::types::{BlockTransactionsKind, Transaction},
    signers::{
        local::{LocalSigner, PrivateKeySigner},
        SignerSync,
    },
};
use anyhow::Error;
use ecdsa::SigningKey;
use k256::Secp256k1;
use serde_json::Value;
use std::str::FromStr;
use std::{sync::Arc, time::Duration};
use tracing::debug;

mod l2_contracts_bindings;
pub mod l2_tx_lists;
pub mod preconf_blocks;

use l2_contracts_bindings::{LibSharedData, TaikoAnchor};
use l2_tx_lists::PendingTxLists;

const GOLDEN_TOUCH_PRIVATE_KEY: &str =
    "92954368afd3caa1f3ce3ead0069c1af414054aefe1ef9aeacc1bf426222ce38";

type WsProvider = FillProvider<
    JoinFill<
        Identity,
        JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
    >,
    RootProvider,
>;

pub struct Taiko {
    taiko_geth_provider_ws: WsProvider,
    taiko_geth_auth_rpc: JSONRPCClient,
    driver_rpc: HttpRPCClient,
    pub chain_id: u64,
    preconfer_address: PreconferAddress,
    ethereum_l1: Arc<EthereumL1>,
    golden_touch_signer: LocalSigner<SigningKey<Secp256k1>>,
    golden_touch_wallet: EthereumWallet,
    taiko_l2_address: Address,
}

impl Taiko {
    pub async fn new(
        taiko_geth_ws_url: &str,
        taiko_geth_auth_url: &str,
        driver_url: &str,
        chain_id: u64,
        rpc_client_timeout: Duration,
        jwt_secret_bytes: &[u8],
        preconfer_address: PreconferAddress,
        ethereum_l1: Arc<EthereumL1>,
        taiko_l2_address: String,
    ) -> Result<Self, Error> {
        let ws = WsConnect::new(taiko_geth_ws_url.to_string());
        let provider_ws = ProviderBuilder::new()
            .on_ws(ws.clone())
            .await
            .map_err(|e| anyhow::anyhow!("Taiko::new: Failed to create WebSocket provider: {e}"))?;

        let signer = PrivateKeySigner::from_str(GOLDEN_TOUCH_PRIVATE_KEY)?;
        let golden_touch_wallet = EthereumWallet::from(signer.clone());

        Ok(Self {
            taiko_geth_provider_ws: provider_ws,
            taiko_geth_auth_rpc: JSONRPCClient::new_with_timeout_and_jwt(
                taiko_geth_auth_url,
                rpc_client_timeout,
                jwt_secret_bytes,
            )?,
            driver_rpc: HttpRPCClient::new_with_jwt(
                driver_url,
                rpc_client_timeout,
                jwt_secret_bytes,
            )?,
            chain_id,
            preconfer_address,
            ethereum_l1,
            golden_touch_signer: signer,
            golden_touch_wallet,
            taiko_l2_address: Address::from_str(&taiko_l2_address)?,
        })
    }

    pub async fn get_pending_l2_tx_lists_from_taiko_geth(&self) -> Result<PendingTxLists, Error> {
        // TODO: adjust following parameters
        let params = vec![
            Value::String(format!("0x{}", hex::encode(self.preconfer_address))), // beneficiary address
            Value::from(0x1dfd14000u64), // baseFee TODO: get it from contract, for now it's 8 gwei
            Value::Number(30_000_000.into()), // blockMaxGasLimit
            Value::Number(131_072.into()), // maxBytesPerTxList (128KB)
            Value::Array(vec![]),        // locals (empty array)
            Value::Number(1.into()),     // maxTransactionsLists
            Value::Number(0.into()),     // minTip
        ];

        let result = self
            .taiko_geth_auth_rpc
            .call_method("taikoAuth_txPoolContentWithMinTip", params)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get L2 tx lists: {}", e))?;
        if result != Value::Null {
            let tx_lists = l2_tx_lists::decompose_pending_lists_json_from_geth(result)
                .map_err(|e| anyhow::anyhow!("Failed to decompose L2 tx lists: {}", e))?;
            Ok(tx_lists)
        } else {
            Ok(vec![])
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

    async fn get_latest_l2_block_id_hash_and_gas_used(&self) -> Result<(u64, B256, u64), Error> {
        let block = self
            .taiko_geth_provider_ws
            .get_block_by_number(BlockNumberOrTag::Latest, BlockTransactionsKind::Hashes)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get latest L2 block: {}", e))?
            .ok_or(anyhow::anyhow!("Failed to get latest L2 block"))?;
        Ok((
            block.header.number(),
            block.header.hash,
            block.header.gas_used(),
        ))
    }

    async fn get_base_fee(&self, l2_head_number: u64) -> Result<u64, Error> {
        // l2Head, err := c.L2.HeaderByNumber(ctx, nil)
        // if err != nil {
        //     return nil, err
        // }

        // baseFee, err := c.CalculateBaseFee(
        //     ctx,
        //     l2Head,
        //     chainConfig.IsPacaya(new(big.Int).Add(l2Head.Number, common.Big1)),
        //     baseFeeConfig,
        //     uint64(

        Ok(0)
    }

    pub async fn advance_head_to_new_l2_blocks(
        &self,
        tx_lists: PendingTxLists,
    ) -> Result<(), Error> {
        tracing::debug!("Submitting new L2 blocks to the Taiko driver");

        for tx_list in tx_lists {
            debug!("processing {} txs", tx_list.tx_list.len());
            let (parent_block_id, parent_hash, parent_gas_used) =
                self.get_latest_l2_block_id_hash_and_gas_used().await?;

            // Safe conversion with overflow check
            let parent_gas_used_u32 = u32::try_from(parent_gas_used).map_err(|_| {
                anyhow::anyhow!("parent_gas_used {} exceeds u32 max value", parent_gas_used)
            })?;

            let anchor_tx = self
                .construct_anchor_tx(parent_block_id, parent_hash, parent_gas_used_u32)
                .await?;
            let tx_list = std::iter::once(anchor_tx)
                .chain(tx_list.tx_list.into_iter())
                .collect::<Vec<_>>();

            let tx_list_bytes = l2_tx_lists::encode_and_compress(&tx_list)?;
            let extra_data = vec![0u8];

            let executable_data = preconf_blocks::ExecutableData {
                base_fee_per_gas: 8_000_000_000u64, // 8 gwei
                block_number: parent_block_id,
                extra_data: format!("0x{}", hex::encode(extra_data)),
                fee_recipient: format!("0x{}", hex::encode(self.preconfer_address)),
                gas_limit: 30_000_000u64,
                parent_hash: format!("0x{}", hex::encode(parent_hash)),
                timestamp: chrono::Utc::now().timestamp() as u64,
                transactions: format!("0x{}", hex::encode(tx_list_bytes)),
            };

            let request_body = preconf_blocks::BuildPreconfBlockRequestBody {
                executable_data,
                signature: "".to_string(),
            };

            // Use the DirectHttpClient to send the request directly
            const API_ENDPOINT: &str = "preconfBlocks";

            let response = self
                .driver_rpc
                .post_json(API_ENDPOINT, &request_body)
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Failed to build preconf block for API '{}': {}",
                        API_ENDPOINT,
                        e
                    )
                })?;

            debug!("preconfBlocks response: {:?}", response);
        }
        Ok(())
    }

    pub async fn construct_anchor_tx(
        &self,
        anchor_block_id: u64,
        anchor_state_root: B256,
        parent_gas_used: u32,
    ) -> Result<Transaction, Error> {
        let config = self.ethereum_l1.execution_layer.get_pacaya_config();

        let base_fee_config = LibSharedData::BaseFeeConfig {
            adjustmentQuotient: config.baseFeeConfig.adjustmentQuotient,
            sharingPctg: config.baseFeeConfig.sharingPctg,
            gasIssuancePerSecond: config.baseFeeConfig.gasIssuancePerSecond,
            minGasExcess: config.baseFeeConfig.minGasExcess,
            maxGasIssuancePerBlock: config.baseFeeConfig.maxGasIssuancePerBlock,
        };

        // Create contract instance with the correct address
        let contract = TaikoAnchor::new(self.taiko_l2_address, &self.taiko_geth_provider_ws);

        // Create the contract call
        let call_builder = contract
            .anchorV3(
                anchor_block_id,
                anchor_state_root,
                parent_gas_used,
                base_fee_config,
                vec![],
            )
            .gas(1_000_000) // value expected by Taiko geth
            .max_fee_per_gas(8_000_000_000u128) // 8 gwei
            .max_priority_fee_per_gas(8_000_000_000u128) // 8 gwei
            .nonce(
                self.taiko_geth_provider_ws
                    .get_transaction_count(self.golden_touch_signer.address())
                    .await?,
            )
            .chain_id(self.chain_id);

        let tx_envelope = call_builder
            .into_transaction_request()
            .build(&self.golden_touch_wallet)
            .await?;

        debug!("transaction type: {:?}", tx_envelope.tx_type());

        let tx = Transaction {
            inner: tx_envelope,
            from: self.golden_touch_signer.address(),
            block_hash: None,
            block_number: None,
            transaction_index: None,
            effective_gas_price: None,
        };
        Ok(tx)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::utils::rpc_server::test::RpcServer;
    use std::net::SocketAddr;

    // TODO: fix this test
    // #[tokio::test]
    // async fn test_get_pending_l2_tx_lists() {
    //     let (mut rpc_server, taiko) = setup_rpc_server_and_taiko(3030).await;
    //     let json = taiko
    //         .get_pending_l2_tx_lists_from_taiko_geth()
    //         .await
    //         .unwrap();

    //     assert_eq!(json.len(), 1);
    //     assert_eq!(json[0].tx_list.len(), 2);
    //     rpc_server.stop().await;
    // }

    // TODO: fix this test
    // #[tokio::test]
    // async fn test_advance_head_to_new_l2_block() {
    //     let (mut rpc_server, taiko) = setup_rpc_server_and_taiko(3040).await;
    //     let value = serde_json::json!({
    //         "TxLists": [
    //             [
    //                 {
    //                     "type": "0x0",
    //                     "chainId": "0x28c61",
    //                     "nonce": "0x1",
    //                     "to": "0xbfadd5365bb2890ad832038837115e60b71f7cbb",
    //                     "gas": "0x267ac",
    //                     "gasPrice": "0x5e76e0800",
    //                     "maxPriorityFeePerGas": null,
    //                     "maxFeePerGas": null,
    //                     "value": "0x0",
    //                     "input": "0x40d097c30000000000000000000000004cea2c7d358e313f5d0287c475f9ae943fe1a913",
    //                     "v": "0x518e6",
    //                     "r": "0xb22da5cdc4c091ec85d2dda9054aa497088e55bd9f0335f39864ae1c598dd35",
    //                     "s": "0x6eee1bcfe6a1855e89dd23d40942c90a036f273159b4c4fd217d58169493f055",
    //                     "hash": "0x7c76b9906579e54df54fe77ad1706c47aca706b3eb5cfd8a30ccc3c5a19e8ecd"
    //                 }
    //             ]
    //         ]
    //     });

    //     let response = taiko.advance_head_to_new_l2_blocks(value).await.unwrap();
    //     rpc_server.stop().await;
    // }

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
}
