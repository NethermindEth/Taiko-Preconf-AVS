use crate::bls::BLSService;
use alloy::hex::encode;
use serde::ser::Serializer;
use serde::Serialize;
use ssz_derive::Encode;
use std::sync::Arc;
//use ssz_derive::{Decode, Encode};
use alloy::{
    consensus::TxEnvelope,
    eips::eip2718::{Decodable2718, Eip2718Result},
    signers::k256::sha2::{Digest, Sha256},
};

#[derive(Debug, Clone, Serialize, Eq, PartialEq, Encode)]
pub struct ConstraintsMessage {
    #[serde(serialize_with = "serialize_data_as_hex")]
    pub pubkey: [u8; 48],
    pub slot: u64,
    pub top: bool,
    pub transactions: Vec<Vec<u8>>,
}

impl ConstraintsMessage {
    pub fn new(pubkey: [u8; 48], slot: u64, transactions: Vec<Vec<u8>>) -> Self {
        ConstraintsMessage {
            pubkey,
            slot,
            top: true,
            transactions,
        }
    }
    /// Returns the digest of this message.
    pub fn digest(&self) -> Eip2718Result<[u8; 32]> {
        let mut hasher = Sha256::new();
        hasher.update(self.pubkey);
        hasher.update(self.slot.to_le_bytes());
        hasher.update((self.top as u8).to_le_bytes());

        for bytes in &self.transactions {
            let tx = TxEnvelope::decode_2718(&mut bytes.as_ref())?;
            hasher.update(tx.tx_hash());
        }

        Ok(hasher.finalize().into())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SignedConstraints {
    pub message: ConstraintsMessage,
    #[serde(serialize_with = "serialize_data1_as_hex")]
    pub signature: [u8; 96],
}

fn serialize_data_as_hex<S>(data: &[u8; 48], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let hex_string = format!("0x{}", encode(data));
    serializer.serialize_str(&hex_string)
}

fn serialize_data1_as_hex<S>(data: &[u8; 96], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let hex_string = format!("0x{}", encode(data));
    serializer.serialize_str(&hex_string)
}

impl SignedConstraints {
    pub fn new(message: ConstraintsMessage, bls: Arc<BLSService>) -> Self {
        // TODO fix data calculation
        let digest = message.digest().expect("Could not compute digest");
        // Use propper DST
        let dst = "BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_"
            .as_bytes()
            .to_vec();
        // Sign message
        let signature: [u8; 96] = bls
            .sign(&digest.to_vec(), &dst)
            .try_into()
            .expect("Vec should have exactly 96 elements");
        Self { message, signature }
    }
}