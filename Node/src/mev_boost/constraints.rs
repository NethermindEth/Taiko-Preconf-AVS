use crate::bls::BLSService;
use alloy::signers::k256::sha2::{Digest, Sha256};
use ethereum_consensus::crypto::PublicKey;
use ethereum_consensus::primitives::BlsSignature;
use ethereum_consensus::state_transition::Context;
use reth_primitives::PooledTransactionsElement;
use serde::Serialize;
use std::sync::Arc;
use tree_hash::TreeHash;
use tree_hash_derive::TreeHash;

pub const GENESIS_VALIDATORS_ROOT: [u8; 32] = [0; 32];
pub const COMMIT_BOOST_DOMAIN: [u8; 4] = [109, 109, 111, 67];

#[derive(Debug, Clone, Serialize, Eq, PartialEq)]
pub struct ConstraintsMessage {
    pub pubkey: PublicKey,
    pub slot: u64,
    pub top: bool,
    pub transactions: Vec<Vec<u8>>,
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

    fn digest(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.pubkey.to_vec());
        hasher.update(self.slot.to_le_bytes());
        hasher.update((self.top as u8).to_le_bytes());
        for tx in self.transactions.iter() {
            // Convert the opaque bytes to a EIP-2718 envelope and obtain the tx hash.
            // this is needed to handle type 3 transactions.
            // FIXME: don't unwrap here and handle the error properly
            let tx = PooledTransactionsElement::decode_enveloped(tx.to_vec().into()).unwrap();
            hasher.update(tx.hash().as_slice());
        }

        hasher.finalize().into()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SignedConstraints {
    pub message: ConstraintsMessage,
    pub signature: BlsSignature,
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

    pub fn new(message: ConstraintsMessage, bls: Arc<BLSService>) -> Self {
        let genesis_fork_version = [16, 0, 0, 56]; // TODO get frpm eth/v1/beacon/genesis. Replace with the correct value for your chain
        let mut context = Context::for_minimal();
        context.genesis_fork_version = genesis_fork_version;

        let digest = message.digest();

        let domain = Self::compute_domain_custom(&context, COMMIT_BOOST_DOMAIN);
        let signing_root = Self::compute_signing_root_custom(digest.tree_hash_root().0, domain);

        // Sign message
        let signature = bls
            .sign_with_ethereum_secret_key(&signing_root)
            .expect("sign_with_domain error");

        Self { message, signature }
    }
}
