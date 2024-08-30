use crate::bls::BLSService;
use alloy::hex::encode;
use anyhow::Error;
use serde::ser::{SerializeSeq, Serializer};
use serde::Serialize;
use ssz::Encode;
use ssz_derive::{Decode, Encode};
use std::sync::Arc;

#[derive(PartialEq, Debug, Encode, Decode, Serialize)]
pub struct ConstraintsMessage {
    validator_index: u64,
    slot: u64,
    top: bool,
    #[serde(serialize_with = "serialize_vec_as_hex")]
    constraints: Vec<Vec<u8>>,
}

impl ConstraintsMessage {
    pub fn new(validator_index: u64, slot: u64, top: bool, constraints: Vec<Vec<u8>>) -> Self {
        Self {
            validator_index,
            slot,
            top,
            constraints,
        }
    }
}

#[derive(Serialize)]
pub struct SignedConstraints {
    message: ConstraintsMessage,
    #[serde(serialize_with = "serialize_data_as_hex")]
    signature: [u8; 192],
}

fn serialize_vec_as_hex<S>(data: &Vec<Vec<u8>>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut seq = serializer.serialize_seq(Some(data.len()))?;
    for vec in data {
        // Convert each Vec<u8> to a hex string prefixed with "0x"
        let hex_string = format!("0x{}", encode(vec));
        seq.serialize_element(&hex_string)?;
    }
    seq.end()
}

fn serialize_data_as_hex<S>(data: &[u8; 192], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let hex_string = format!("0x{}", encode(data));
    serializer.serialize_str(&hex_string)
}

impl SignedConstraints {
    pub fn new(message: ConstraintsMessage, bls: Arc<BLSService>) -> Self {
        let data = message.merkle_root().unwrap();
        // TODO check signature
        let signature = bls.sign(&data, &[]).serialize();
        Self { message, signature }
    }
}

impl ConstraintsMessage {
    pub fn merkle_root(&self) -> Result<[u8; 32], Error> {
        let ssz_message: Vec<u8> = self.as_ssz_bytes();
        let merkle_root = tree_hash::merkle_root(&ssz_message, 0);
        Ok(merkle_root.into())
    }
}
