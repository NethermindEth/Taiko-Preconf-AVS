use crate::utils::preconfirmation_proof::PreconfirmationProof;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreconfirmationMessage {
    pub block_height: u64,
    pub tx_lists: Value,
    pub tx_list_bytes: Vec<u8>,
    pub gas_used: u64,
    pub proof: PreconfirmationProof,
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
