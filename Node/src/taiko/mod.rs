use crate::utils::rpc_client::RpcClient;
use anyhow::Error;
use serde_json::Value;

pub struct Taiko {
    rpc_client: RpcClient,
}

impl Taiko {
    pub fn new(url: &str) -> Self {
        Self {
            rpc_client: RpcClient::new(url),
        }
    }

    pub async fn get_pending_l2_tx_lists(&self) -> Result<Value, Error> {
        tracing::debug!("Getting L2 tx lists");
        self.rpc_client
            .call_method("RPC.GetL2TxLists", vec![])
            .await
    }

    pub async fn submit_new_l2_blocks(&self, value: Value) -> Result<Value, Error> {
        tracing::debug!("Submitting new L2 blocks");

        self.rpc_client
            .call_method("RPC.AdvanceL2ChainHeadWithNewBlocks", vec![value])
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
        tracing_subscriber::fmt::init();

        // Start the RPC server
        let mut rpc_server = RpcServer::new();
        let addr: SocketAddr = "127.0.0.1:3030".parse().unwrap();
        rpc_server.start_test_responses(addr).await.unwrap();

        let taiko = Taiko::new("http://127.0.0.1:3030");
        let json = taiko.get_pending_l2_tx_lists().await.unwrap();

        assert_eq!(json["result"]["TxLists"].as_array().unwrap().len(), 1);
        assert_eq!(json["result"]["TxLists"][0].as_array().unwrap().len(), 3);
        assert_eq!(json["result"]["TxLists"][0][0]["type"], "0x0");
        assert_eq!(
            json["result"]["TxLists"][0][0]["hash"],
            "0x7c76b9906579e54df54fe77ad1706c47aca706b3eb5cfd8a30ccc3c5a19e8ecd"
        );
        assert_eq!(json["result"]["TxLists"][0][1]["type"], "0x2");
        assert_eq!(
            json["result"]["TxLists"][0][1]["hash"],
            "0xece2a3c6ca097cfe5d97aad4e79393240f63865210f9c763703d1136f065298b"
        );
        assert_eq!(json["result"]["TxLists"][0][2]["type"], "0x2");
        assert_eq!(
            json["result"]["TxLists"][0][2]["hash"],
            "0xb105d9f16e8fb913093c8a2c595bf4257328d256f218a05be8dcc626ddeb4193"
        );
        rpc_server.stop().await;
    }

    #[tokio::test]
    async fn test_submit_new_l2_blocks() {
        tracing_subscriber::fmt::init();

        // Start the RPC server
        let mut rpc_server = RpcServer::new();
        let addr: SocketAddr = "127.0.0.1:3030".parse().unwrap();
        rpc_server.start_test_responses(addr).await.unwrap();

        let taiko = Taiko::new("http://127.0.0.1:3030");
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

        let response = taiko.submit_new_l2_blocks(value).await.unwrap();
        assert_eq!(response["result"], "Request received and processed successfully");
        rpc_server.stop().await;
    }
}
