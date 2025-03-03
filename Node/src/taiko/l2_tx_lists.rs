use alloy::rpc::types::Transaction;
use alloy_rlp::{Decodable, Encodable, RlpDecodable, RlpEncodable};
use anyhow::Error;
use flate2::{write::ZlibEncoder, Compression};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::io::Write;

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
pub struct PreBuiltTxList {
    pub tx_list: Vec<Transaction>,
    estimated_gas_used: u64,
    bytes_length: u64,
}

// RLP encode and zlib compress
pub fn encode_and_compress(tx_list: &[Transaction]) -> Result<Vec<u8>, Error> {
    // First RLP encode the transactions
    let mut buffer = Vec::<u8>::new();
    alloy_rlp::encode_iter(tx_list.iter().map(|tx| tx.inner.clone()), &mut buffer);

    // Then compress using zlib
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&buffer)
        .map_err(|e| anyhow::anyhow!("PreBuiltTxList::encode: Failed to compress: {}", e))?;
    encoder
        .finish()
        .map_err(|e| anyhow::anyhow!("PreBuiltTxList::encode: Failed to finish: {}", e))
}

pub type PendingTxLists = Vec<PreBuiltTxList>;

pub fn decompose_pending_lists_json_from_geth(json: Value) -> Result<PendingTxLists, Error> {
    let rpc_reply: PendingTxLists = serde_json::from_value(json)?;
    Ok(rpc_reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_pending_tx_lists() {
        let pending_tx_lists = serde_json::from_str::<PendingTxLists>(include_str!(
            "../utils/tx_lists_test_response_from_geth.json"
        ))
        .unwrap();

        println!("{:?}", pending_tx_lists);

        assert_eq!(pending_tx_lists.len(), 1);
        assert_eq!(pending_tx_lists[0].tx_list.len(), 2);
        let tx_legacy = pending_tx_lists[0].tx_list[0].inner.as_legacy().unwrap();
        assert_eq!(tx_legacy.tx().chain_id, Some(167001));
        assert_eq!(
            pending_tx_lists[0].tx_list[1].from,
            "0x8943545177806ed17b9f23f0a21ee5948ecaa776"
                .parse::<alloy::primitives::Address>()
                .unwrap()
        );
        assert_eq!(pending_tx_lists[0].estimated_gas_used, 42000);
        assert_eq!(pending_tx_lists[0].bytes_length, 203);
    }
}
