use alloy::rpc::types::Transaction;
use anyhow::Error;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct RPCReplyL2TxLists {
    pub tx_lists: Value, // TODO: decode and create tx_list_bytes on AVS node side
    #[serde(deserialize_with = "deserialize_tx_lists_bytes")]
    pub tx_list_bytes: Vec<Vec<u8>>,
    #[serde(deserialize_with = "deserialize_parent_meta_hash")]
    pub parent_meta_hash: [u8; 32],
    #[serde(rename = "ParentBlockID")]
    pub parent_block_id: u64,
}

fn deserialize_tx_lists_bytes<'de, D>(deserializer: D) -> Result<Vec<Vec<u8>>, D::Error>
where
    D: Deserializer<'de>,
{
    let vec: Vec<String> = Deserialize::deserialize(deserializer)?;
    let result = vec
        .iter()
        .map(|s| s.as_bytes().to_vec())
        .collect::<Vec<Vec<u8>>>();
    Ok(result)
}

fn deserialize_parent_meta_hash<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    let s = s.trim_start_matches("0x");
    let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
    if bytes.len() != 32 {
        return Err(serde::de::Error::custom(
            "Invalid length for parent_meta_hash",
        ));
    }
    let mut array = [0u8; 32];
    array.copy_from_slice(&bytes);
    Ok(array)
}

pub fn decompose_pending_lists_json(json: Value) -> Result<RPCReplyL2TxLists, Error> {
    // Deserialize the JSON string into the struct
    let rpc_reply: RPCReplyL2TxLists = serde_json::from_value(json)?;
    Ok(rpc_reply)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
struct PreBuiltTxList {
    #[serde(deserialize_with = "deserialize_tx_list")]
    pub tx_list: Vec<Transaction>,
    estimated_gas_used: u64,
    bytes_length: u64,
}

fn deserialize_tx_list<'de, D>(deserializer: D) -> Result<Vec<Transaction>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    let transactions = value
        .as_array()
        .ok_or_else(|| serde::de::Error::custom("Expected array"))?
        .iter()
        .map(|tx| {
            let tx_envelope = serde_json::from_value::<alloy::consensus::TxEnvelope>(tx.clone())
                .map_err(|e| {
                    serde::de::Error::custom(format!("Failed to parse transaction: {}", e))
                })?;
            let signer = tx_envelope.recover_signer().map_err(|e| {
                serde::de::Error::custom(format!("Failed to recover signer: {}", e))
            })?;
            Ok(Transaction {
                inner: tx_envelope,
                block_hash: None,
                block_number: None,
                transaction_index: None,
                effective_gas_price: None,
                from: signer,
            })
        })
        .collect::<Result<Vec<Transaction>, D::Error>>()?;
    Ok(transactions)
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "PascalCase")]
pub struct PendingTxLists(Vec<PreBuiltTxList>);

pub fn decompose_pending_lists_json_from_geth(json: Value) -> Result<PendingTxLists, Error> {
    let rpc_reply: PendingTxLists = serde_json::from_value(json)?;
    Ok(rpc_reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decompose_pending_lists_json() {
        let json_data =
            serde_json::from_str(include_str!("../utils/tx_lists_test_response.json")).unwrap();

        let result = decompose_pending_lists_json(json_data).unwrap();

        assert_eq!(result.tx_lists.as_array().unwrap().len(), 1);
        assert_eq!(
            result.tx_lists.as_array().unwrap()[0]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(result.tx_list_bytes.len(), 1);
        assert_eq!(result.tx_list_bytes[0].len(), 492);
        assert_eq!(result.parent_meta_hash.len(), 32);
        assert_eq!(result.parent_block_id, 1234);
    }

    #[test]
    fn test_deserialize_pending_tx_lists() {
        let json_data = serde_json::from_str(include_str!(
            "../utils/tx_lists_test_response_from_geth.json"
        ))
        .unwrap();
        let pending_tx_lists = PendingTxLists::from(json_data);

        println!("{:?}", pending_tx_lists);

        assert_eq!(pending_tx_lists.0.len(), 1);
        assert_eq!(pending_tx_lists.0[0].tx_list.len(), 2);
        let tx_legacy = pending_tx_lists.0[0].tx_list[0].inner.as_legacy().unwrap();
        assert_eq!(tx_legacy.tx().chain_id, Some(167000));
        assert_eq!(
            pending_tx_lists.0[0].tx_list[1].from,
            "0xe25583099ba105d9ec0a67f5ae86d90e50036425"
                .parse::<alloy::primitives::Address>()
                .unwrap()
        );
        assert_eq!(pending_tx_lists.0[0].estimated_gas_used, 42000);
        assert_eq!(pending_tx_lists.0[0].bytes_length, 203);
    }
}
