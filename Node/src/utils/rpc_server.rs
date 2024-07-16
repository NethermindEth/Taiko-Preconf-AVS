#[cfg(test)]
pub mod test {
    use jsonrpsee::server::{ServerBuilder, ServerHandle};
    use jsonrpsee::RpcModule;
    use serde_json::json;
    use std::net::SocketAddr;
    use tracing::info;

    pub struct RpcServer {
        handle: Option<ServerHandle>,
    }

    impl RpcServer {
        pub fn new() -> Self {
            RpcServer {
                handle: None::<ServerHandle>,
            }
        }

        #[cfg(test)]
        pub async fn start_test_responses(
            &mut self,
            addr: SocketAddr,
        ) -> Result<(), Box<dyn std::error::Error>> {
            let server = ServerBuilder::default().build(addr).await?;
            let mut module = RpcModule::new(());

            module.register_async_method("RPC.GetL2TxLists", |_, _, _| async {
                let tx_lists_response: serde_json::Value =
                    serde_json::from_str(include_str!("tx_lists_test_response.json")).unwrap();
                tx_lists_response
            })?;
            module.register_async_method(
                "RPC.AdvanceL2ChainHeadWithNewBlocks",
                |_, _, _| async {
                    json!({
                        "result": "Request received and processed successfully",
                        "id": 1
                    })
                },
            )?;

            let handle = server.start(module);
            tokio::spawn(handle.clone().stopped());

            self.handle = Some(handle);
            Ok(())
        }

        pub async fn stop(&mut self) {
            if let Some(handle) = self.handle.take() {
                handle.stop().unwrap();
            }
            info!("Server stopped");
        }
    }
}
