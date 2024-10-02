use crate::utils::types::ECDSASignature;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreconfirmationProof {
    pub commit_hash: [u8; 32],
    #[serde(with = "serde_bytes")]
    pub signature: ECDSASignature,
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

//test
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_preconfirmation_proof() {
        let commit_hash: [u8; 32] = [1; 32];
        let signature: ECDSASignature = [2; 65];

        let preconfirmation_proof = PreconfirmationProof{
            commit_hash,
            signature
        };

        let bytes: Vec<u8> = preconfirmation_proof.clone().into();
        let preconfirmation_proof_restore: PreconfirmationProof = bytes.into();
        assert_eq!(preconfirmation_proof_restore.commit_hash, preconfirmation_proof.commit_hash);
        assert_eq!(preconfirmation_proof_restore.signature, preconfirmation_proof.signature);
    }
}
