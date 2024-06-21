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

    pub fn submit_new_l2_blocks(&self) {
        tracing::debug!("Submitting new L2 blocks");
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

        assert_eq!(json["result"]["TxLists"][0][0]["type"], "0x0");
        assert_eq!(json["result"]["TxLists"][0][0]["chainId"], "0x28c61");
        rpc_server.stop().await;
    }
}
