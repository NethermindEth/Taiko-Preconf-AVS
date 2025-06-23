use crate::utils::rpc_client::JSONRPCClient;
use anyhow::Error;
use hex;
use serde_json::Value;
use std::time::Duration;

pub struct Web3Signer {
    client: JSONRPCClient,
}

impl Web3Signer {
    pub fn new(rpc_url: &str, timeout: Duration) -> Result<Self, Error> {
        let client = JSONRPCClient::new_with_timeout(rpc_url, timeout)?;
        Ok(Self { client })
    }

    pub async fn sign(&self, source_address: &str, tx: &Vec<u8>) -> Result<Vec<u8>, Error> {
        let response = self
            .client
            .call_method(
                "eth_sign",
                vec![
                    Value::String(format!("0x{}", source_address)),
                    Value::String(format!("0x{}", hex::encode(tx))),
                ],
            )
            .await
            .map_err(|e| anyhow::anyhow!("Web3Signer Failed to sign transaction: {}", e))?;

        if let Some(signature) = response.as_str().map(|s| s.strip_prefix("0x").unwrap_or(s)) {
            return Ok(hex::decode(signature)
                .map_err(|e| anyhow::anyhow!("Web3Signer Failed to decode signature: {}", e))?);
        }

        Err(anyhow::anyhow!(
            "Web3Signer Failed to sign transaction: {}",
            response
        ))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use mockito;

    #[tokio::test]
    async fn test_sign() {
        tracing::subscriber::set_global_default(
            tracing_subscriber::FmtSubscriber::builder()
                .with_max_level(tracing::Level::TRACE)
                .finish(),
        )
        .unwrap();

        let server = setup_web3signer_rpc_server().await;
        let web3signer = Web3Signer::new(server.url().as_str(), Duration::from_secs(1)).unwrap();
        let signature = web3signer
            .sign(
                "d7dF738C3a6963f25F02285FAd15814baC21dbE1",
                &vec![0x2e, 0xad, 0xbe, 0x1f],
            )
            .await
            .unwrap();
        assert_eq!(
            signature,
            vec![
                0xd5, 0x88, 0x06, 0xb4, 0xe9, 0xbb, 0x4a, 0x54, 0x83, 0xf8, 0x44, 0x91, 0x92, 0xa5,
                0x14, 0x3c, 0x3d, 0xf7, 0x48, 0x29, 0xa0, 0x0a, 0x3a, 0x66, 0x66, 0xe5, 0xd9, 0xe0,
                0xca, 0x95, 0x55, 0x70, 0x44, 0x10, 0xc8, 0x5b, 0xad, 0xa4, 0xc8, 0x2e, 0xe1, 0xea,
                0x1a, 0x4a, 0xba, 0x67, 0xe9, 0x35, 0xc6, 0x60, 0x56, 0xad, 0xea, 0xaa, 0xc9, 0xbd,
                0x54, 0xe9, 0xcd, 0x76, 0x0e, 0x2d, 0x2a, 0x2b, 0x1b
            ]
        );
    }

    #[tokio::test]
    async fn test_wrong_address() {
        let server = setup_web3signer_rpc_server().await;
        let web3signer = Web3Signer::new(server.url().as_str(), Duration::from_secs(1)).unwrap();
        let signature = web3signer
            .sign(
                "d7dF738C3a6963f25F02285FAd15814baC21dbE2",
                &vec![0x2e, 0xad, 0xbe, 0x1f],
            )
            .await;

        assert!(signature.is_err());
    }

    async fn setup_web3signer_rpc_server() -> mockito::ServerGuard {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/")
            .match_body(r#"{"jsonrpc":"2.0","id":0,"method":"eth_sign","params":["0xd7dF738C3a6963f25F02285FAd15814baC21dbE1","0x2eadbe1f"]}"#)
            .with_body(r#"
             {"jsonrpc":"2.0","id":0,"result":"0xd58806b4e9bb4a5483f8449192a5143c3df74829a00a3a6666e5d9e0ca9555704410c85bada4c82ee1ea1a4aba67e935c66056adeaaac9bd54e9cd760e2d2a2b1b"}
            "#)
            .create();

        server
        .mock("POST", "/")
        .match_body(r#"{"jsonrpc":"2.0","id":0,"method":"eth_sign","params":["0xd7dF738C3a6963f25F02285FAd15814baC21dbE2","0x2eadbe1f"]}"#)
        .with_body(r#"
         { code: ServerError(-32000), message: "No unlocked account matches the Sender", data: None }
        "#)
        .create();

        server
    }
}
