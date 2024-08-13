use anyhow::Error;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "PascalCase")]
pub struct BlockProposed {
    #[serde(rename = "BlockID")]
    pub block_id: u64,
    pub tx_list_hash: [u8; 32],
    #[serde(deserialize_with = "deserialize_proposer")]
    pub proposer: [u8; 20],
}

fn deserialize_proposer<'de, D>(deserializer: D) -> Result<[u8; 20], D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    let s = s.trim_start_matches("0x");
    let bytes = hex::decode(s).map_err(serde::de::Error::custom)?;
    if bytes.len() != 20 {
        return Err(serde::de::Error::custom(
            "Invalid length for proposer address",
        ));
    }
    let mut array = [0u8; 20];
    array.copy_from_slice(&bytes);
    Ok(array)
}

pub fn decompose_block_proposed_json(json_data: Value) -> Result<BlockProposed, Error> {
    let block_proposed: BlockProposed = serde_json::from_value(json_data)?;
    Ok(block_proposed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_decompose_block_proposed_json() {
        let json_data = json!({
            "BlockID":4321,"TxListHash":[12,34,56,78,90,12,34,56,78,90,12,34,56,78,90,12,34,56,78,90,12,34,56,78,90,12,34,56,78,90,12,34],"Proposer":"0x0000000000000000000000000000000000000008"
        });

        let result = decompose_block_proposed_json(json_data).unwrap();

        assert_eq!(result.block_id, 4321);
        assert_eq!(
            result.tx_list_hash,
            [
                12, 34, 56, 78, 90, 12, 34, 56, 78, 90, 12, 34, 56, 78, 90, 12, 34, 56, 78, 90, 12,
                34, 56, 78, 90, 12, 34, 56, 78, 90, 12, 34,
            ]
        );
        assert_eq!(
            result.proposer,
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8,]
        );
    }
}
