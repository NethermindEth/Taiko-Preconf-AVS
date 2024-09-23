use crate::utils::types::*;
use crate::{taiko::l2_tx_lists::RPCReplyL2TxLists, utils::bytes_tools::hash_bytes_with_keccak};
use alloy_rlp::{Encodable, RlpDecodable, RlpEncodable};
use anyhow::Error;
use secp256k1::{ecdsa::Signature, Message, Secp256k1, SecretKey};

//https://github.com/NethermindEth/Taiko-Preconf-AVS/blob/caf9fbbde0dd84947af5a7b26610ffd38525d932/SmartContracts/src/avs/PreconfTaskManager.sol#L175
#[derive(Debug, RlpEncodable, RlpDecodable, PartialEq)]
pub struct L2TxListsCommit {
    pub block_height: [u8; 32],
    pub chain_id: [u8; 32],
    pub tx_list_hash: [u8; 32],
}

impl L2TxListsCommit {
    pub fn new(reply: &RPCReplyL2TxLists, block_height: u64, chain_id: u64) -> Self {
        let block_height_bytes = block_height.to_le_bytes(); // Convert u64 to a [u8; 8] array
        let mut block_height = [0u8; 32];
        block_height[24..].copy_from_slice(&block_height_bytes);
        let chain_id_bytes = chain_id.to_le_bytes(); // Convert u64 to a [u8; 8] array
        let mut chain_id = [0u8; 32];
        chain_id[24..].copy_from_slice(&chain_id_bytes);
        L2TxListsCommit {
            block_height,
            chain_id,
            tx_list_hash: hash_bytes_with_keccak(reply.tx_list_bytes[0].as_slice()),
        }
    }

    pub fn from_preconf(block_height: u64, tx_list_hash: L2TxListHash, chain_id: u64) -> Self {
        let block_height_bytes = block_height.to_le_bytes(); // Convert u64 to a [u8; 8] array
        let mut block_height = [0u8; 32];
        block_height[24..].copy_from_slice(&block_height_bytes);
        let chain_id_bytes = chain_id.to_le_bytes(); // Convert u64 to a [u8; 8] array
        let mut chain_id = [0u8; 32];
        chain_id[24..].copy_from_slice(&chain_id_bytes);
        L2TxListsCommit {
            block_height,
            chain_id,
            tx_list_hash,
        }
    }
}

impl L2TxListsCommit {
    pub fn hash(&self) -> Result<[u8; 32], Error> {
        let mut buffer = Vec::<u8>::new();
        self.encode(&mut buffer);
        Ok(hash_bytes_with_keccak(&buffer.as_slice()))
    }

    #[allow(dead_code)]
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
            tx_list_hash: [1u8; 32],
            chain_id: [0u8; 32],
            block_height: [0u8; 32],
        };

        let hash_result = commit.hash();
        assert!(hash_result.is_ok());
        let hash = hash_result.unwrap();
        assert_eq!(hash.len(), 32);
    }

    #[test]
    fn test_sign() {
        let mut block_height = [0u8; 32];
        block_height[31] = 1;
        let commit = L2TxListsCommit {
            tx_list_hash: [1u8; 32],
            chain_id: [0u8; 32],
            block_height,
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
