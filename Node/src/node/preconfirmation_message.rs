use super::preconfirmation_proof::PreconfirmationProof;
use crate::utils::{bytes_tools::hash_bytes_with_keccak, types::*};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreconfirmationMessage {
    pub block_height: u64,
    #[serde(with = "serde_json_as_string")]
    pub tx_lists: Value,
    pub tx_list_hash: L2TxListHash,
    pub proof: PreconfirmationProof,
}

impl PreconfirmationMessage {
    pub fn new(
        block_height: u64,
        tx_lists: Value,
        tx_list_rlp_bytes: &Vec<u8>,
        proof: PreconfirmationProof,
    ) -> Self {
        PreconfirmationMessage {
            block_height,
            tx_lists,
            tx_list_hash: hash_bytes_with_keccak(tx_list_rlp_bytes.as_slice()),
            proof,
        }
    }
}

mod serde_json_as_string {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use serde_json::{Value, to_string, from_str};

    pub fn serialize<S>(value: &Value, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let s = to_string(value).map_err(serde::ser::Error::custom)?;
        serializer.serialize_str(&s)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl From<PreconfirmationMessage> for Vec<u8> {
    fn from(val: PreconfirmationMessage) -> Self {
        bincode::serialize(&val).expect("Serialization failed")
    }
}

impl From<Vec<u8>> for PreconfirmationMessage {
    fn from(bytes: Vec<u8>) -> Self {
        bincode::deserialize(&bytes).expect("Deserialization failed")
    }
}

//test
#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    #[test]
    fn test_preconfirmation_message() {
        let block_height: u64 = 1;
        let tx_lists = json!("{value1: 1, value2: 2}");
        let tx_list_rlp_bytes = vec![1, 2, 3, 4];
        let proof = PreconfirmationProof {
            commit_hash: [3; 32],
            signature: [4; 65],
        };
        let preconfirmation_message = PreconfirmationMessage::new(
            block_height,
            tx_lists,
            &tx_list_rlp_bytes,
            proof.clone(),
        );
        
        let bytes: Vec<u8> = preconfirmation_message.clone().into();
        let preconfirmation_message2 = PreconfirmationMessage::from(bytes);
        assert_eq!(preconfirmation_message2.block_height, preconfirmation_message.block_height);
        assert_eq!(preconfirmation_message2.tx_lists, preconfirmation_message.tx_lists);
        assert_eq!(preconfirmation_message2.tx_list_hash, preconfirmation_message.tx_list_hash);
        assert_eq!(preconfirmation_message2.proof.commit_hash, preconfirmation_message.proof.commit_hash);
        assert_eq!(preconfirmation_message2.proof.signature, preconfirmation_message.proof.signature);
    }
}
