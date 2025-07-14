use crate::utils::rpc_client::JSONRPCClient;
use alloy::consensus::TxType;
use alloy::{primitives::TxKind, rpc::types::TransactionRequest};
use anyhow::Error;
use hex;
use serde_json::{Map, Value};
use std::time::Duration;
use tracing::info;

pub struct Web3Signer {
    client: JSONRPCClient,
}

impl Web3Signer {
    pub async fn new(
        rpc_url: &str,
        timeout: Duration,
        signer_address: &str,
    ) -> Result<Self, Error> {
        let client = JSONRPCClient::new_with_timeout(rpc_url, timeout)?;
        Self::check_web3signer_version(&client).await?;
        if !Self::is_signer_key_available(&client, signer_address).await? {
            return Err(anyhow::anyhow!(
                "Web3Signer: Signer key is not available for address {}",
                signer_address
            ));
        }
        Ok(Self { client })
    }

    async fn check_web3signer_version(client: &JSONRPCClient) -> Result<(), Error> {
        let response = client
            .call_method_with_retry("health_status", vec![])
            .await
            .map_err(|e| anyhow::anyhow!("Web3Signer: Failed to get health status: {}", e))?;
        let version = response.as_str().ok_or(anyhow::anyhow!(
            "Web3Signer: Failed to decode health status"
        ))?;
        info!(
            "Web3Signer available at {} with version {}",
            client.url(),
            version
        );
        Ok(())
    }

    async fn is_signer_key_available(
        client: &JSONRPCClient,
        signer_address: &str,
    ) -> Result<bool, Error> {
        let response = client
            .call_method_with_retry("eth_accounts", vec![])
            .await
            .map_err(|e| anyhow::anyhow!("Web3Signer: Failed to get available accounts: {}", e))?;
        let accounts = response.as_array().ok_or(anyhow::anyhow!(
            "Web3Signer: Failed to decode available accounts"
        ))?;
        Ok(accounts.contains(&Value::String(signer_address.to_string())))
    }

    pub async fn sign_transaction(&self, tx: TransactionRequest) -> Result<Vec<u8>, Error> {
        tracing::debug!(
            "Web3Signer signing transaction, source_address: {:?}",
            tx.from,
        );

        let tx_type = tx.buildable_type().ok_or(anyhow::anyhow!(
            "Web3Signer: Transaction is not any of the supported types (EIP-1559, EIP-4844)"
        ))?;

        // Construct transaction object similar to the provided JSON structure
        let mut tx_obj = Map::new();
        tx_obj.insert(
            "from".to_string(),
            Value::String(
                tx.from
                    .ok_or(anyhow::anyhow!("Web3Signer: Transaction from is not set"))?
                    .to_string(),
            ),
        );
        let to = match tx.to {
            Some(to) => match to {
                TxKind::Create => {
                    return Err(anyhow::anyhow!("Web3Signer: Transaction to is not set"));
                }
                TxKind::Call(to) => to.to_string(),
            },
            None => return Err(anyhow::anyhow!("Web3Signer: Transaction to is not set")),
        };
        tx_obj.insert("to".to_string(), Value::String(to));
        tx_obj.insert(
            "gas".to_string(),
            Value::String(
                tx.gas
                    .ok_or(anyhow::anyhow!("Web3Signer: Transaction gas is not set"))?
                    .to_string(),
            ),
        );
        tx_obj.insert(
            "nonce".to_string(),
            Value::String(
                tx.nonce
                    .ok_or(anyhow::anyhow!("Web3Signer: Transaction nonce is not set"))?
                    .to_string(),
            ),
        );
        if let Some(chain_id) = tx.chain_id {
            tx_obj.insert("chainId".to_string(), Value::String(chain_id.to_string()));
        }
        if let Some(input) = tx.input.input {
            tx_obj.insert("data".to_string(), Value::String(hex::encode(input)));
        }
        if let Some(value) = tx.value {
            tx_obj.insert("value".to_string(), Value::String(value.to_string()));
        }

        if tx_type == TxType::Eip1559 || tx_type == TxType::Eip4844 {
            tx_obj.insert(
                "maxPriorityFeePerGas".to_string(),
                Value::String(
                    tx.max_priority_fee_per_gas
                        .ok_or(anyhow::anyhow!(
                            "Web3Signer: Transaction max_priority_fee_per_gas is not set"
                        ))?
                        .to_string(),
                ),
            );
            tx_obj.insert(
                "maxFeePerGas".to_string(),
                Value::String(
                    tx.max_fee_per_gas
                        .ok_or(anyhow::anyhow!(
                            "Web3Signer: Transaction max_fee_per_gas is not set"
                        ))?
                        .to_string(),
                ),
            );
        }

        if tx_type == TxType::Eip4844 {
            tx_obj.insert(
                "maxFeePerBlobGas".to_string(),
                Value::String(
                    tx.max_fee_per_blob_gas
                        .ok_or(anyhow::anyhow!(
                            "Web3Signer: Transaction max_fee_per_blob_gas is not set"
                        ))?
                        .to_string(),
                ),
            );

            if let Some(sidecar) = tx.sidecar {
                let commitments = sidecar
                    .versioned_hashes()
                    .map(|h| Value::String(hex::encode(h)))
                    .collect::<Vec<_>>();
                tx_obj.insert("blobVersionedHashes".to_string(), Value::Array(commitments));
            } else {
                return Err(anyhow::anyhow!(
                    "Web3Signer: Transaction sidecar is not set for EIP-4844 transaction"
                ));
            }
        }

        let response = self
            .client
            .call_method_with_retry("eth_signTransaction", vec![Value::Object(tx_obj)])
            .await
            .map_err(|e| anyhow::anyhow!("Web3Signer: Failed to sign transaction: {}", e))?;

        if let Some(signature) = response.as_str().map(|s| s.strip_prefix("0x").unwrap_or(s)) {
            return hex::decode(signature)
                .map_err(|e| anyhow::anyhow!("Web3Signer: Failed to decode signature: {}", e));
        }

        Err(anyhow::anyhow!(
            "Web3Signer: Failed to sign transaction: {}",
            response
        ))
    }
}
