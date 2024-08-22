use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreconfirmationProof {
    pub commit_hash: [u8; 32],
    #[serde(with = "serde_bytes")]
    pub signature: [u8; 65], // ECDSA 65 bytes signature
}

impl From<PreconfirmationProof> for Vec<u8> {
    fn from(val: PreconfirmationProof) -> Self {
        bincode::serialize(&val).expect("Serialization failed")
    }
}

impl From<Vec<u8>> for PreconfirmationProof {
    fn from(bytes: Vec<u8>) -> Self {
        bincode::deserialize(&bytes).expect("Deserialization failed")
    }
}