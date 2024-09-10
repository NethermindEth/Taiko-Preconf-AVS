use crate::bls::BLSService;
use alloy::hex::encode;
use serde::ser::Serializer;
use serde::Serialize;
use ssz::Encode;
use ssz_derive::{Decode, Encode};
use std::sync::Arc;

#[derive(PartialEq, Debug, Encode, Decode, Serialize)]
pub struct Constraint {
    #[serde(serialize_with = "serialize_vec_as_hex")]
    tx: Vec<u8>,
    index: Option<u64>,
}

#[derive(PartialEq, Debug, Encode, Decode, Serialize)]
pub struct ConstraintsMessage {
    validator_index: u64,
    slot: u64,
    constraints: Vec<Constraint>,
}

impl ConstraintsMessage {
    pub fn new(validator_index: u64, slot: u64, messages: Vec<Vec<u8>>) -> Self {
        let constraints = messages
            .iter()
            .map(|message| Constraint {
                tx: message.clone(),
                index: Some(0),
            })
            .collect();
        Self {
            validator_index,
            slot,
            constraints,
        }
    }
}

#[derive(Serialize)]
pub struct SignedConstraints {
    message: ConstraintsMessage,
    #[serde(serialize_with = "serialize_data_as_hex")]
    signature: [u8; 96],
}

fn serialize_vec_as_hex<S>(data: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let hex_string = format!("0x{}", encode(data));
    serializer.serialize_str(&hex_string)
}

fn serialize_data_as_hex<S>(data: &[u8; 96], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let hex_string = format!("0x{}", encode(data));
    serializer.serialize_str(&hex_string)
}

impl SignedConstraints {
    pub fn new(message: ConstraintsMessage, bls: Arc<BLSService>) -> Self {
        // Encode message;
        let data = message.as_ssz_bytes();
        // Use propper DST
        let dst = "BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_POP_"
            .as_bytes()
            .to_vec();
        // Sign message
        let signature: [u8; 96] = bls
            .sign(&data, &dst)
            .try_into()
            .expect("Vec should have exactly 96 elements");
        Self { message, signature }
    }
}
