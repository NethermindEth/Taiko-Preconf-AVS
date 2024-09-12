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
}
