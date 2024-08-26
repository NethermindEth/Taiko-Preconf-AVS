use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone)]
pub struct Constraint {
    tx: String,
    index: Option<u64>,
}

impl Constraint {
    pub fn new(tx: String, index: Option<u64>) -> Self {
        Self { tx, index }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ConstraintsMessage {
    validator_index: u64,
    slot: u64,
    constraints: Vec<Constraint>,
}

impl ConstraintsMessage {
    pub fn new(validator_index: u64, slot: u64, constraints: Vec<Constraint>) -> Self {
        Self {
            validator_index,
            slot,
            constraints,
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SignedConstraints {
    message: ConstraintsMessage,
    signature: String,
}

impl SignedConstraints {
    pub fn new(message: ConstraintsMessage, signature: String) -> Self {
        Self { message, signature }
    }
}

impl From<ConstraintsMessage> for Vec<u8> {
    fn from(val: ConstraintsMessage) -> Self {
        bincode::serialize(&val).expect("MEV Boost message serialization failed")
    }
}
