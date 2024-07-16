#![allow(dead_code)] // TODO: remove
use crate::taiko::l2_tx_lists::RPCReplyL2TxLists;
use anyhow::Error;
use secp256k1::{ecdsa::Signature, Message, Secp256k1, SecretKey};
use serde::{Deserialize, Serialize};
use tiny_keccak::{Hasher, Keccak};

#[derive(Serialize, Deserialize)]
pub struct L2TxListsCommit {
    pub tx_list_bytes: Vec<u8>,
    pub parent_meta_hash: [u8; 32],
    pub block_height: u64,
}

impl From<RPCReplyL2TxLists> for L2TxListsCommit {
    fn from(reply: RPCReplyL2TxLists) -> Self {
        L2TxListsCommit {
            tx_list_bytes: reply.tx_list_bytes[0].clone(), // TODO check for other indexes
            parent_meta_hash: reply.parent_meta_hash,
            block_height: 1, //TODO add to the replay
        }
    }
}

impl L2TxListsCommit {
    pub fn hash(&self) -> Result<[u8; 32], Error> {
        let serialized = serde_json::to_vec(&self)?;
        let mut hasher = Keccak::v256();
        hasher.update(&serialized);
        let mut result = [0u8; 32];
        hasher.finalize(&mut result);
        Ok(result)
    }

    pub fn sign(&self, private_key: &str) -> Result<Signature, Error> {
        let secp = Secp256k1::new();
        let secret_key = SecretKey::from_slice(&hex::decode(private_key)?)?;
        let message = Message::from_digest_slice(&self.hash()?)?;
        let signature = secp.sign_ecdsa(&message, &secret_key);
        Ok(signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash() {
        let commit = L2TxListsCommit {
            tx_list_bytes: vec![1, 2, 3, 4, 5],
            parent_meta_hash: [0u8; 32],
            block_height: 1,
        };

        let hash_result = commit.hash();
        assert!(hash_result.is_ok());
        let hash = hash_result.unwrap();
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_sign() {
        let commit = L2TxListsCommit {
            tx_list_bytes: vec![1, 2, 3, 4, 5],
            parent_meta_hash: [0u8; 32],
            block_height: 1,
        };

        let private_key = "c87509a1c067bbde78beb793e6fa950b8d9c7f7bd5a8b16bf0d3a1a5b9bdfd3b";
        let sign_result = commit.sign(private_key);
        assert!(sign_result.is_ok());

        let signature = sign_result.unwrap();
        let secp = Secp256k1::new();
        let public_key = SecretKey::from_slice(&hex::decode(private_key).unwrap())
            .unwrap()
            .public_key(&secp);
        let message = Message::from_digest_slice(&commit.hash().unwrap()).unwrap();
        assert!(secp.verify_ecdsa(&message, &signature, &public_key).is_ok());
    }
}
