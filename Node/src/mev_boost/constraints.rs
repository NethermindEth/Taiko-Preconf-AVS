use crate::bls::BLSService;
use alloy::signers::k256::sha2::{Digest, Sha256};
use anyhow::Error;
use blst::min_pk::{PublicKey, Signature};
use ethereum_consensus::state_transition::Context;
use reth_primitives::PooledTransactionsElement;
use serde::ser::Serializer;
use serde::Serialize;
use std::sync::Arc;
use tree_hash::TreeHash;
use tree_hash_derive::TreeHash;

pub const GENESIS_VALIDATORS_ROOT: [u8; 32] = [0; 32];
pub const COMMIT_BOOST_DOMAIN: [u8; 4] = [109, 109, 111, 67];
const BLS_DST: &[u8] = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_";

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct ConstraintsMessage {
    #[serde(serialize_with = "serialize_publickey")]
    pub pubkey: PublicKey,
    pub slot: u64,
    pub top: bool,
    pub transactions: Vec<Vec<u8>>,
}
fn serialize_publickey<S>(data: &PublicKey, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let hex_string = format!("0x{}", alloy::hex::encode(data.compress()));
    serializer.serialize_str(&hex_string)
}

impl ConstraintsMessage {
    pub fn new(pubkey: PublicKey, slot: u64, transactions: Vec<Vec<u8>>) -> Self {
        ConstraintsMessage {
            pubkey,
            slot,
            top: true,
            transactions,
        }
    }

    fn digest(&self) -> Result<[u8; 32], Error> {
        let mut hasher = Sha256::new();
        hasher.update(self.pubkey.compress());
        hasher.update(self.slot.to_le_bytes());
        hasher.update((self.top as u8).to_le_bytes());
        for tx in self.transactions.iter() {
            // Convert the opaque bytes to a EIP-2718 envelope and obtain the tx hash.
            // this is needed to handle type 3 transactions.
            let tx = PooledTransactionsElement::decode_enveloped(tx.to_vec().into())?;
            hasher.update(tx.hash().as_slice());
        }

        Ok(hasher.finalize().into())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SignedConstraints {
    pub message: ConstraintsMessage,
    #[serde(serialize_with = "serialize_signature")]
    pub signature: Signature,
}

fn serialize_signature<S>(data: &Signature, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let hex_string = format!("0x{}", alloy::hex::encode(data.compress()));
    serializer.serialize_str(&hex_string)
}

impl SignedConstraints {
    pub fn compute_domain_custom(chain: &Context, domain_mask: [u8; 4]) -> [u8; 32] {
        #[derive(Debug, TreeHash)]
        struct ForkData {
            fork_version: [u8; 4],
            genesis_validators_root: [u8; 32],
        }

        let mut domain = [0u8; 32];
        domain[..4].copy_from_slice(&domain_mask);

        let fork_version = chain.genesis_fork_version;
        let fd = ForkData {
            fork_version,
            genesis_validators_root: GENESIS_VALIDATORS_ROOT,
        };
        let fork_data_root = fd.tree_hash_root().0;

        domain[4..].copy_from_slice(&fork_data_root[..28]);

        domain
    }

    pub fn compute_signing_root_custom(
        object_root: [u8; 32],
        signing_domain: [u8; 32],
    ) -> [u8; 32] {
        #[derive(Default, Debug, TreeHash)]
        struct SigningData {
            object_root: [u8; 32],
            signing_domain: [u8; 32],
        }

        let signing_data = SigningData {
            object_root,
            signing_domain,
        };
        signing_data.tree_hash_root().0
    }

    pub fn new(
        message: ConstraintsMessage,
        bls: Arc<BLSService>,
        genesis_fork_version: [u8; 4],
    ) -> Result<Self, Error> {
        // Prepare data
        let mut context = Context::for_minimal();
        context.genesis_fork_version = genesis_fork_version;

        let digest = message.digest()?;

        let domain = Self::compute_domain_custom(&context, COMMIT_BOOST_DOMAIN);
        let signing_root = Self::compute_signing_root_custom(digest.tree_hash_root().0, domain);

        // Sign message
        let signature = bls.sign(&signing_root, BLS_DST);

        Ok(Self { message, signature })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constraints_message_digest() {
        let constraints = vec![vec![2,249,3,213,131,48,24,36,6,132,59,154,202,0,133,4,168,23,200,0,131,15,66,64,148,96,100,247,86,247,243,220,130,128,193,207,160,28,228,26,55,181,241,109,241,128,185,3,100,242,118,107,125,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,128,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,2,96,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,28,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,3,64,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,192,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,97,69,97,210,209,67,98,30,18,110,135,131,26,239,40,118,120,180,66,184,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,96,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,1,128,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,32,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,188,101,74,119,65,102,81,67,67,47,47,104,55,117,72,107,67,43,72,97,68,65,111,120,89,65,89,82,51,78,90,81,65,104,81,98,56,73,54,119,65,103,119,77,48,85,74,82,83,107,97,85,53,70,48,101,70,43,116,121,84,55,47,54,99,110,79,116,54,86,72,71,97,53,73,99,86,85,80,102,99,112,119,65,65,103,77,65,66,111,75,114,75,88,82,69,106,104,98,106,115,115,104,112,50,119,104,83,102,54,47,117,79,101,70,110,69,82,56,113,57,52,99,80,119,103,120,116,108,51,110,79,78,111,67,82,115,53,98,70,52,108,87,85,113,101,118,122,121,109,106,50,75,81,71,74,106,57,117,87,54,107,110,74,101,85,86,68,112,47,81,43,83,65,49,78,74,65,81,65,65,47,47,56,97,57,84,57,51,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,192,1,160,175,154,114,57,217,247,104,167,114,56,70,166,250,175,140,155,4,255,254,185,177,119,184,80,186,72,127,32,38,28,186,156,160,63,77,214,119,203,52,47,233,31,158,227,102,147,186,40,117,153,98,44,15,194,109,222,72,181,48,135,170,91,85,51,123]];
        let slot_id = 17;
        let pubkey = PublicKey::from_bytes(&alloy::hex::decode("908d6f98b5eaf6ac1b632c6b80b304612d48afd9c104874f9025960accdae128028119608b0d95a7e141390101fba669").unwrap()).unwrap();

        let message = ConstraintsMessage::new(pubkey, slot_id, constraints);

        assert_eq!(message.digest().unwrap(),[9, 87, 0, 71, 187, 129, 0, 133, 126, 114, 244, 187, 129, 105, 194, 105, 195, 115, 27, 220, 144, 157, 88, 34, 184, 108, 130, 34, 84, 248, 88, 125]);
    }

    #[test]
    fn test_signed_constraints_compute_domain_custom() {
        let mut context = Context::for_minimal();
        context.genesis_fork_version = [16, 0, 0, 56];

        let domain = SignedConstraints::compute_domain_custom(&context, COMMIT_BOOST_DOMAIN);

        assert_eq!(domain, [109, 109, 111, 67, 11, 65, 190, 76, 219, 52, 209, 131, 221, 220, 165, 57, 131, 55, 98, 109, 205, 207, 175, 23, 32, 193, 32, 45, 59, 149, 248, 78]);
    }

    #[test]
    fn test_signed_constraints_compute_signing_root_custom() {
        let digest = [9, 87, 0, 71, 187, 129, 0, 133, 126, 114, 244, 187, 129, 105, 194, 105, 195, 115, 27, 220, 144, 157, 88, 34, 184, 108, 130, 34, 84, 248, 88, 125];
        let domain = [109, 109, 111, 67, 11, 65, 190, 76, 219, 52, 209, 131, 221, 220, 165, 57, 131, 55, 98, 109, 205, 207, 175, 23, 32, 193, 32, 45, 59, 149, 248, 78];

        let signing_root = SignedConstraints::compute_signing_root_custom(digest.tree_hash_root().0, domain);

        assert_eq!(signing_root, [46, 115, 119, 45, 23, 162, 89, 198, 203, 58, 165, 97, 28, 21, 0, 117, 149, 27, 106, 219, 120, 115, 115, 227, 114, 157, 221, 247, 183, 14, 65, 54]);
    }
}