use crate::utils::rpc_client::RpcClient;
use anyhow::Error;
use serde_json::Value;

pub mod l2_tx_lists;

pub struct Taiko {
    rpc_proposer: RpcClient,
    rpc_driver: RpcClient,
    pub chain_id: u64,
}

impl Taiko {
    pub fn new(proposer_url: &str, driver_url: &str, chain_id: u64) -> Self {
        Self {
            rpc_proposer: RpcClient::new(proposer_url),
            rpc_driver: RpcClient::new(driver_url),
            chain_id,
        }
    }

    pub async fn get_pending_l2_tx_lists(&self) -> Result<l2_tx_lists::RPCReplyL2TxLists, Error> {
        tracing::debug!("Getting L2 tx lists");
        let result = l2_tx_lists::decompose_pending_lists_json(
            self.rpc_proposer
                .call_method("RPC.GetL2TxLists", vec![])
                .await?,
        )?;

        if !result.tx_list_bytes.is_empty() {
            Self::print_number_of_received_txs(&result);
        }

        Ok(result)
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
        tracing::debug!("Submitting new L2 blocks");
        let payload = serde_json::json!({
            "TxLists": tx_lists,
            "gasUsed": 0u64,    //TODO remove here and in the driver
        });
        self.rpc_driver
            .call_method("RPC.AdvanceL2ChainHeadWithNewBlocks", vec![payload])
            .await
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
        );
        (rpc_server, taiko)
    }
}
