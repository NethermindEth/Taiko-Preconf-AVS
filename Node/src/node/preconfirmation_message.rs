use super::preconfirmation_proof::PreconfirmationProof;
use crate::utils::{bytes_tools::hash_bytes_with_keccak, types::*};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreconfirmationMessage {
    pub block_height: u64,
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
