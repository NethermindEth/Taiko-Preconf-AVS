#![allow(unused)] // TODO: remove this once using new rpc functions

use crate::utils::{rpc_client::RpcClient, types::*};
use anyhow::Error;
use serde_json::Value;
use std::time::Duration;
use tracing::debug;

pub mod l2_tx_lists;

pub struct Taiko {
    rpc_taiko_geth: RpcClient,
    rpc_driver: RpcClient,
    pub chain_id: u64,
    preconfer_address: PreconferAddress,
}

impl Taiko {
    pub fn new(
        taiko_geth_url: &str,
        driver_url: &str,
        chain_id: u64,
        rpc_client_timeout: Duration,
        jwt_secret_bytes: &[u8],
        preconfer_address: PreconferAddress,
    ) -> Result<Self, Error> {
        Ok(Self {
            rpc_taiko_geth: RpcClient::new_with_timeout_and_jwt(
                taiko_geth_url,
                rpc_client_timeout,
                jwt_secret_bytes,
            )?,
            rpc_driver: RpcClient::new(driver_url),
            chain_id,
            preconfer_address,
        })
    }

    // TODO: obsolete, remove this function
    pub async fn get_pending_l2_tx_lists(&self) -> Result<l2_tx_lists::RPCReplyL2TxLists, Error> {
        tracing::debug!("Getting L2 tx lists");
        let result = l2_tx_lists::decompose_pending_lists_json(
            self.rpc_taiko_geth
                .call_method("RPC.GetL2TxLists", vec![])
                .await
                .map_err(|e| anyhow::anyhow!("Failed to get L2 tx lists: {}", e))?,
        )
        .map_err(|e| anyhow::anyhow!("Failed to decompose L2 tx lists: {}", e))?;

        if !result.tx_list_bytes.is_empty() {
            Self::print_number_of_received_txs(&result);
            debug!(
                "Parent meta hash: 0x{}",
                hex::encode(result.parent_meta_hash)
            );
            debug!("Parent block id: {}", result.parent_block_id);
        }

        Ok(result)
    }

    pub async fn get_pending_l2_txs_from_taiko_geth(
        &self,
    ) -> Result<l2_tx_lists::PendingTxLists, Error> {
        let params = vec![
            Value::String(format!("0x{}", hex::encode(self.preconfer_address))), // beneficiary address
            Value::from(0x1dfd14000u64), // baseFee (8 gwei) - now as a number, not a string
            Value::Number(30_000_000.into()), // blockMaxGasLimit
            Value::Number(131_072.into()), // maxBytesPerTxList (128KB)
            Value::Array(vec![]),        // locals (empty array)
            Value::Number(1.into()),     // maxTransactionsLists
            Value::Number(0.into()),     // minTip
        ];

        let result = self
            .rpc_taiko_geth
            .call_method("taikoAuth_txPoolContentWithMinTip", params)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get L2 tx lists: {}", e))?;

        let tx_lists = l2_tx_lists::decompose_pending_lists_json_from_geth(result)
            .map_err(|e| anyhow::anyhow!("Failed to decompose L2 tx lists: {}", e))?;
        Ok(tx_lists)
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

    pub async fn advance_head_to_new_l2_block(&self, tx_lists: Value) -> Result<Value, Error> {
        tracing::debug!("Submitting new L2 blocks to the Taiko driver");
        let payload = serde_json::json!({
            "TxLists": tx_lists,
            "gasUsed": 0u64,    //TODO remove here and in the driver
        });
        self.rpc_driver
            .call_method("RPC.AdvanceL2ChainHeadWithNewBlocks", vec![payload])
            .await
            .map_err(|e| anyhow::anyhow!("Failed to advance L2 chain head with new blocks: {}", e))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::utils::rpc_server::test::RpcServer;
    use std::net::SocketAddr;

    #[tokio::test]
    async fn test_get_pending_l2_tx_lists() {
        let (mut rpc_server, taiko) = setup_rpc_server_and_taiko(3030).await;
        let json = taiko.get_pending_l2_tx_lists().await.unwrap().tx_lists;

        assert_eq!(json.as_array().unwrap().len(), 1);
        assert_eq!(json[0].as_array().unwrap().len(), 2);
        assert_eq!(json[0][0]["type"], "0x0");
        assert_eq!(
            json[0][0]["hash"],
            "0xc653e446eafe51eea1f46e6e351adbd1cc8a3271e6935f1441f613a58d441f6a"
        );
        assert_eq!(json[0][1]["type"], "0x2");
        assert_eq!(
            json[0][1]["hash"],
            "0xffbcd2fab90f1bf314ca2da1bf83eeab3d17fd58a0393d29a697b2ff05d0e65c"
        );
        rpc_server.stop().await;
    }

    #[tokio::test]
    async fn test_advance_head_to_new_l2_block() {
        let (mut rpc_server, taiko) = setup_rpc_server_and_taiko(3040).await;
        let value = serde_json::json!({
            "TxLists": [
                [
                    {
                        "type": "0x0",
                        "chainId": "0x28c61",
                        "nonce": "0x1",
                        "to": "0xbfadd5365bb2890ad832038837115e60b71f7cbb",
                        "gas": "0x267ac",
                        "gasPrice": "0x5e76e0800",
                        "maxPriorityFeePerGas": null,
                        "maxFeePerGas": null,
                        "value": "0x0",
                        "input": "0x40d097c30000000000000000000000004cea2c7d358e313f5d0287c475f9ae943fe1a913",
                        "v": "0x518e6",
                        "r": "0xb22da5cdc4c091ec85d2dda9054aa497088e55bd9f0335f39864ae1c598dd35",
                        "s": "0x6eee1bcfe6a1855e89dd23d40942c90a036f273159b4c4fd217d58169493f055",
                        "hash": "0x7c76b9906579e54df54fe77ad1706c47aca706b3eb5cfd8a30ccc3c5a19e8ecd"
                    }
                ]
            ]
        });

        let response = taiko.advance_head_to_new_l2_block(value).await.unwrap();
        assert_eq!(
            response["result"],
            "Request received and processed successfully"
        );
        rpc_server.stop().await;
    }

    async fn setup_rpc_server_and_taiko(port: u16) -> (RpcServer, Taiko) {
        // Start the RPC server
        let mut rpc_server = RpcServer::new();
        let addr: SocketAddr = format!("127.0.0.1:{}", port).parse().unwrap();
        rpc_server.start_test_responses(addr).await.unwrap();

        let taiko = Taiko::new(
            &format!("http://127.0.0.1:{}", port),
            &format!("http://127.0.0.1:{}", port),
            1,
            Duration::from_secs(10),
            &[
                0xa6, 0xea, 0x92, 0x58, 0xca, 0x91, 0x2c, 0x59, 0x3b, 0x3e, 0x36, 0xee, 0x36, 0xc1,
                0x7f, 0xe9, 0x74, 0x47, 0xf9, 0x20, 0xf5, 0xb3, 0x6a, 0x90, 0x74, 0x4d, 0x79, 0xd4,
                0xf2, 0xd6, 0xae, 0x62,
            ],
            PRECONFER_ADDRESS_ZERO,
        )
        .unwrap();
        (rpc_server, taiko)
    }
}
